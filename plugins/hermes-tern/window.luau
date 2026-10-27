--!strict
-- Registration only: loading this package never starts a process.
-- Tern's launch.command is shell interpreted. Keep it fixed: never interpolate
-- pane titles, prompts, paths, actor messages or model output into this string.
tern.command({
    id = "chat",
    title = "Hermes: Open conversation",
    run = function(cx)
        local pane = cx.layout:new_tab({ command = "hermes-tern open" })
        if pane == nil then
            cx:toast("error", "Could not open Hermes conversation")
        end
    end,
})

tern.command({
    id = "preview",
    title = "Hermes: Open SDK preview (no agent)",
    run = function(cx)
        local pane = cx.layout:new_tab({ command = "hermes-tern preview" })
        if pane == nil then
            cx:toast("error", "Could not open Hermes preview")
        end
    end,
})

-- VM-local consent, durable Rust dispatch fence. Neither is an OMP receipt.
local active: any = nil
local function uuid(value: any): boolean
    return type(value) == "string" and #value == 36
        and string.match(value, "^[0-9a-f]+%-[0-9a-f]+%-[0-9a-f]+%-[0-9a-f]+%-[0-9a-f]+$") ~= nil
        and string.sub(value, 9, 9) == "-" and string.sub(value, 14, 14) == "-"
        and string.sub(value, 19, 19) == "-" and string.sub(value, 24, 24) == "-"
        and value ~= "00000000-0000-0000-0000-000000000000"
end
local function optional(value: any): boolean
    return value == nil or value == tern.json.null
end
local function path(value: any): boolean
    return type(value) == "string" and #value <= 4096 and string.sub(value, 1, 1) == "/"
        and string.find(value, "%c") == nil
end
local function proposal(value: any, id: string): boolean
    return type(value) == "table" and value.v == 1 and value.handoff_id == id
        and uuid(value.request_id) and uuid(value.attempt_id) and uuid(value.target_id)
        and type(value.digest) == "string" and #value.digest == 64 and string.match(value.digest, "^[0-9a-f]+$") ~= nil
        and type(value.text) == "string" and #value.text > 0 and #value.text <= 8192
        and type(value.source) == "string" and #value.source > 0 and #value.source <= 256
end
local function same(a: any, b: any): boolean
    if not proposal(b, a.handoff_id) then return false end
    for _, key in {"request_id", "attempt_id", "target_id", "digest", "text", "source"} do
        if a[key] ~= b[key] then return false end
    end
    return true
end
local function live(flow: any): boolean
    return active == flow and tern.now() < flow.deadline
end
local function finish(flow: any, cx: any, message: string)
    if active ~= flow then return end
    active = nil
    if flow.timer then flow.timer:cancel(); flow.timer = nil end
    if cx then cx:toast("info", message, "No automatic resend or task completion") end
end
-- Never retain WindowCx across callbacks: every continuation uses its argument.
local function once(flow: any, fn: any): any
    local used = false
    return function(value: any, err: any, cx: any)
        if used or not live(flow) then return end
        used = true
        if not cx then finish(flow, nil, "Window context unavailable"); return end
        local ok = pcall(fn, value, err, cx)
        if not ok then finish(flow, cx, "Handoff stopped; any armed attempt remains unknown") end
    end
end
local function exchange(flow: any, op: any, cx: any, fn: any)
    if not live(flow) then return end
    local callback = once(flow, function(result: any, _err: any, fresh: any)
        if type(result) ~= "table" or result.status ~= 0 or result.timed_out ~= false
            or type(result.stdout) ~= "string" or #result.stdout > 65536 then
            finish(flow, fresh, "Handoff exchange failed; inspect retained state"); return
        end
        local decoded, reply = pcall(tern.json.decode, result.stdout)
        if not decoded or type(reply) ~= "table" or reply.v ~= 1
            or reply.request_id ~= flow.id or reply.op ~= op.type
            or not proposal(reply.proposal, flow.id)
            or (flow.proposal and not same(flow.proposal, reply.proposal)) then
            finish(flow, fresh, "Handoff response identity mismatch"); return
        end
        fn(reply, fresh)
    end)
    local ok = pcall(function()
        tern.process.run({flow.binary, "omp-handoff", "exchange", "--state", flow.state}, {
            stdin = tern.json.encode({v = 1, request_id = flow.id, op = op}), timeout_ms = 8000,
        }, function(result: any, fresh: any) callback(result, nil, fresh) end)
    end)
    if not ok then finish(flow, cx, "Could not run the configured handoff executable") end
end
local function observe(flow: any, event: string, cx: any, fn: any)
    local p = flow.proposal
    exchange(flow, {type = "record", handoff_id = p.handoff_id, attempt_id = p.attempt_id,
        target_id = p.target_id, event = event}, cx, function(reply: any, fresh: any)
        if reply.event ~= event then finish(flow, fresh, "Invalid observation response"); return end
        fn(fresh)
    end)
end
local function emptyLayer(layer: any): boolean
    if type(layer) ~= "table" or layer.id ~= "layer" or layer.k ~= "col"
        or (layer.more ~= nil and layer.more ~= 0) then return false end
    if layer.p ~= nil and (type(layer.p) ~= "table" or next(layer.p) ~= nil) then return false end
    return layer.c == nil or (type(layer.c) == "table" and next(layer.c) == nil)
end
-- Conservative suitability, not durable session authentication. Surfaces may be
-- retained; queued/submitted/idle are observations, never completion receipts.
local function target(cx: any, pane: number): any
    local localMember = false
    for _, host in cx.hosts:list() do
        if host.kind == "local" and host.state == "connected" then
            for _, id in host.panes do if id == pane then localMember = true end end
        end
    end
    if not localMember then return nil end
    local agent = nil
    for _, candidate in cx.agents:list() do
        if candidate.pane == pane then
            if agent then return nil end
            agent = candidate
        end
    end
    if not agent or agent.state ~= "idle" or not optional(agent.delivery_error) then return nil end
    local info = cx.session:pane(pane)
    if not info or info.kind ~= "agent" or info.busy ~= false or not optional(info.exited)
        or not optional(info.error) or type(info.cwd) ~= "string" or info.cwd == "" then return nil end
    local tree = cx.session:surface(pane, {as = "table", depth = 64, nodes = 5000, max_chars = 64000})
    if type(tree) ~= "table" or type(tree.surface) ~= "table" or tree.surface.role ~= "omp.session"
        or type(tree.surface.id) ~= "string" or tree.surface.mode ~= "inline"
        or type(tree.dock) ~= "table" or tree.dock.id ~= "dock" or tree.dock.k ~= "col"
        or not emptyLayer(tree.layer) or (tree.more ~= nil and tree.more ~= 0) then return nil end
    local editors, composers, count, selected, invalid = 0, 0, 0, nil, false
    local function walk(node: any, inside: boolean, inDock: boolean, depth: number)
        if node == nil then return end
        count += 1
        if type(node) ~= "table" or depth > 64 or count > 5000 or (node.more ~= nil and node.more ~= 0) then
            invalid = true; return
        end
        local props = node.p or {}
        local role = props.role
        if role == "omp.editor" then
            if not inDock then invalid = true end
            composers += 1; inside = true
        end
        if type(role) == "string" and (role:sub(1, 11) == "omp.editor." or role == "omp.composer.focus"
            or role == "omp.composer.agent" or role == "omp.composer.mode") then invalid = true end
        if node.k == "editor" then
            editors += 1
            if not inDock or not inside or props.sendable ~= true or props.text ~= "" or type(node.id) ~= "string" then invalid = true end
            selected = node.id
        end
        if node.c then for _, child in node.c do walk(child, inside, inDock, depth + 1) end end
    end
    walk(tree.main, false, false, 1); walk(tree.dock, false, true, 1)
    if invalid or editors ~= 1 or composers ~= 1 then return nil end
    return {pane = pane, surface = tree.surface.id, editor = selected, cwd = info.cwd, program = info.program}
end
local function unchanged(cx: any, expected: any): boolean
    local now = target(cx, expected.pane)
    if not now then return false end
    for _, key in {"pane", "surface", "editor", "cwd", "program"} do
        if expected[key] ~= now[key] then return false end
    end
    return true
end
local function send(flow: any, cx: any)
    if not unchanged(cx, flow.target) then finish(flow, cx, "OMP target changed; nothing sent"); return end
    local p = flow.proposal
    exchange(flow, {type = "arm", handoff_id = p.handoff_id, attempt_id = p.attempt_id,
        target_id = p.target_id, digest = p.digest}, cx, function(reply: any, fresh: any)
        if reply.newly_armed ~= true then finish(flow, fresh, "Attempt already fenced; never resend automatically"); return end
        flow.armed = true
        if not unchanged(fresh, flow.target) then
            observe(flow, "outcome_unknown", fresh, function(nextCx: any) finish(flow, nextCx, "Target changed after arm; no send") end)
            return
        end
        if flow.asked then finish(flow, fresh, "Duplicate callback refused"); return end
        flow.asked = true
        local ok = pcall(function() fresh.agents:ask(flow.handle, p.text) end)
        if not ok then
            observe(flow, "outcome_unknown", fresh, function(nextCx: any) finish(flow, nextCx, "Ask outcome unknown; inspect OMP") end)
            return
        end
        observe(flow, "ask_returned", fresh, function(nextCx: any)
            nextCx.agents:wait(flow.handle, {timeout = 30000}):next(once(flow, function(_text: any, err: any, waitCx: any)
                local event = err ~= nil and "outcome_unknown" or "idle_observed"
                observe(flow, event, waitCx, function(doneCx: any)
                    finish(flow, doneCx, event == "idle_observed" and "OMP idle observed; result not verified" or "OMP outcome unknown")
                end)
            end))
        end)
    end)
end
local function review(cx: any, p: any, targetPane: number): number?
    local marker = "hermes-tern/omp-handoff-review/v1"
    local view = tern.ui.col({tern.ui.text("Untrusted source label:"), tern.ui.code(p.source),
        tern.ui.text("Exact prompt for selected local OMP pane " .. tostring(targetPane) .. ":"),
        tern.ui.code(p.text), tern.ui.text("Sending may invoke OMP tools; this is not approval or a sandbox."),
        tern.ui.text("Surface data may be retained. Composer/session identity is not locked by this API."),
        tern.ui.text("This authorizes one pane-level ask, which may queue for later delivery. Do not switch the target session."),
        tern.ui.text("Reload or lost response remains unknown. No retry.")})
    local owned = nil
    for _, canvas in cx.canvas:list() do
        if canvas.owner == tern.plugin.id and canvas.about == marker then
            if owned then return nil end
            owned = canvas.pane
        end
    end
    if owned then
        cx.canvas:set(owned, {title = "Review exact OMP handoff", about = marker, view = view})
        cx.layout:focus(owned)
        return owned
    end
    return cx.canvas:open({title = "Review exact OMP handoff", how = "beside", focus = true,
        about = marker, view = view})
end
local function choose(flow: any, cx: any)
    local items = {}
    for _, agent in cx.agents:list() do
        if #items >= 64 then finish(flow, cx, "Too many local targets; narrow the window first"); return end
        local snapshot = target(cx, agent.pane)
        if snapshot then table.insert(items, {label = "Local OMP pane " .. tostring(agent.pane), target = snapshot}) end
    end
    if #items == 0 then finish(flow, cx, "No idle local native OMP with an empty sendable composer"); return end
    cx:pick({items = items, placeholder = "Select an existing local OMP pane"}):next(once(flow, function(item: any, err: any, fresh: any)
        if err or not item or not table.find(items, item) or not unchanged(fresh, item.target) then
            finish(flow, fresh, "Handoff cancelled or target changed"); return
        end
        flow.target = item.target
        flow.handle = fresh.agents:get(item.target.pane)
        local pane = review(fresh, flow.proposal, item.target.pane)
        if pane == nil then finish(flow, fresh, "Could not show exact payload; not sending"); return end
        local decisions = {{id = "cancel", label = "Cancel; do not send"},
            {id = "send_once", label = "Ask selected pane once (may queue)"}}
        fresh:pick({items = decisions, placeholder = "Review the full prompt, then explicitly send once"}):next(once(flow,
            function(decision: any, pickErr: any, confirmed: any)
                if pickErr or decision ~= decisions[2] then finish(flow, confirmed, "Handoff cancelled"); return end
                send(flow, confirmed)
            end))
    end))
end
tern.command({
    id = "omp_handoff",
    title = "Hermes: Review one OMP handoff",
    run = function(cx)
        if active then cx:toast("info", "A handoff is already open in this window"); return end
        local binary, state = tern.getenv("HERMES_TERN_HANDOFF_BIN"), tern.getenv("HERMES_TERN_HANDOFF_STATE")
        if not path(binary) or not path(state) then cx:toast("error", "Configure explicit handoff binary and state paths"); return end
        local flow: any = {binary = binary, state = state, deadline = tern.now() + 120000}
        active = flow
        flow.timer = tern.timer(120000, function(fresh) finish(flow, fresh, "Handoff expired; inspect any armed attempt before proceeding") end)
        cx:prompt({placeholder = "Exact handoff UUID from the Rust offer response"}):next(once(flow, function(id: any, err: any, fresh: any)
            if err or not uuid(id) then finish(flow, fresh, "Handoff cancelled or invalid UUID"); return end
            flow.id = id
            exchange(flow, {type = "prepare", handoff_id = id}, fresh, function(reply: any, nextCx: any)
                if reply.armed ~= false then finish(flow, nextCx, "Attempt already armed; inspect state, never resend"); return end
                flow.proposal = reply.proposal
                choose(flow, nextCx)
            end)
        end))
    end,
})
