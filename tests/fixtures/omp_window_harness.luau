-- Executes the production window entry with synthetic host callbacks, never live consent.
local function exercise(mode, real)
    local commands, pending, records, asks, canvases = {}, {}, {}, {}, {}
    local alive, clock, picks, armed, launches = false, 0, 0, false, 0
    local contextGeneration, expiredUse, reloaded = 0, false, false
    local canvasUpdates, canvasOpens, cancelledTimers = 0, 0, 0
    local proposal = real or {
        v = 1, request_id = "10000000-0000-4000-8000-000000000001",
        handoff_id = "10000000-0000-4000-8000-000000000002",
        attempt_id = "10000000-0000-4000-8000-000000000003",
        target_id = "10000000-0000-4000-8000-000000000004",
        digest = string.rep("a", 64), text = "Review only.\nNo automatic approval.", source = "untrusted source",
    }
    local pane, handle = 17, {pane = 17}
    local callbacks = {}
    local function check() assert(alive, "expired window context used") end
    local function schedule(fn) table.insert(pending, fn) end
    local function awaitable(result, err)
        return {next = function(_, fn)
            schedule(function(cx) fn(result, err, cx) end)
        end}
    end
    -- Pinned OMP Reconciler creates all three region roots, including empty layer.
    local composer = {id = "dock/composer", k = "col", p = {role = "omp.editor"}, c = {
        {id = "dock/composer/line/input", k = "editor", p = {text = "", sendable = true}}}}
    local surface = {surface = {id = "s:1", role = "omp.session", mode = "inline"},
        main = {id = "main", k = "col", c = {}}, dock = {id = "dock", k = "col", c = {composer}},
        layer = {id = "layer", k = "col", c = {}}}
    local agent = {pane = pane, state = "idle", cwd = "/workspace"}
    local host = {kind = "local", state = "connected", panes = {pane}, id = 0}
    if mode == "remote" then host.kind = "remote" end
    if mode == "waiting" then agent.state = "waiting_input" end
    if mode == "shell" then composer.p.role = "omp.editor.bash" end
    if mode == "subagent" then table.insert(composer.c, {id = "focus", k = "row", p = {role = "omp.composer.focus"}}) end
    if mode == "capped" then surface.more = 1 end
    if mode == "not_sendable" then composer.c[1].p.sendable = false end
    if mode == "draft" then composer.c[1].p.text = "unsent draft" end
    if mode == "layer" then surface.layer.c = {{id = "approval", k = "col"}} end
    if mode == "capped_layer" then surface.layer.more = 1 end
    if mode == "unknown_layer" then surface.layer.p = {role = "unknown"} end
    if mode == "retained_main" then surface.main.c = {composer}; surface.dock = nil; surface.layer = nil end
    if mode == "main_editor" then surface.main.c = {composer}; surface.dock.c = {} end
    local cx = {}
    local function viewText(node)
        if type(node) ~= "table" then return "" end
        local text = node.p and node.p.text or ""
        for _, c in node.c or {} do text ..= "\n" .. viewText(c) end
        return text
    end
    local function freshContext()
        contextGeneration += 1
        local generation = contextGeneration
        local function wrap(value)
            if type(value) == "table" then
                local copy = {}
                for k, v in value do copy[k] = wrap(v) end
                return copy
            elseif type(value) == "function" then
                return function(...)
                    if generation ~= contextGeneration or not alive then
                        expiredUse = true
                        error("expired window context used")
                    end
                    return value(...)
                end
            end
            return value
        end
        return wrap(cx)
    end
    cx.hosts = {list = function() check(); return {host} end}
    cx.agents = {
        list = function() check(); return {agent} end,
        get = function(_, p) check(); assert(p == pane); return handle end,
        ask = function(_, h, text)
            check(); assert(h == handle, "lost Agent handle")
            assert(armed and picks >= 2 and #canvases == 1, "ask before durable arm/consent")
            assert(text == proposal.text); table.insert(asks, text)
            if mode == "ask_error" then error("ask ambiguous") end
        end,
        wait = function(_, h, opts)
            check(); assert(h == handle and opts.timeout > 0 and opts.timeout <= 30000)
            return awaitable("UNTRUSTED OUTPUT MUST NOT BE EXPORTED", mode == "wait_error" and "timeout" or nil)
        end,
    }
    cx.session = {
        pane = function(_, p)
            check(); assert(p == pane)
            return {pane = pane, kind = "agent", cwd = "/workspace", program = "omp", busy = false, title = "not identity"}
        end,
        surface = function(_, p, opts)
            check(); assert(p == pane and opts.nodes <= 5000 and opts.depth <= 64)
            return surface
        end,
    }
    cx.toast = function() check() end
    cx.layout = {focus = function(_, p) check(); assert(p == 100) end}
    cx.prompt = function()
        check()
        if mode == "cancel_prompt" then return awaitable(nil) end
        return awaitable(proposal.handoff_id)
    end
    cx.pick = function(_, opts)
        check(); picks += 1
        if picks == 1 then
            if mode == "cancel_target" then return awaitable(nil) end
            return awaitable(opts.items[1])
        end
        local send, cancel
        for _, item in opts.items do
            if item.id == "send_once" then send = item end
            if item.id == "cancel" then cancel = item end
        end
        assert(send and cancel, "explicit send and cancel required")
        assert(string.find(send.label, "may queue", 1, true), "queued delivery risk omitted from consent")
        local shown = viewText(canvases[1].view)
        assert(string.find(shown, "Composer/session identity is not locked", 1, true), "conditional-send guarantee implied")
        assert(string.find(shown, "Surface data may be retained", 1, true), "retained-surface uncertainty hidden")
        return awaitable(mode == "cancel_consent" and cancel or send)
    end
    local existing = mode == "existing_review"
    cx.canvas = {
        list = function()
            check()
            if not existing then return {} end
            return {{pane = 100, owner = "hermes-tern", about = "hermes-tern/omp-handoff-review/v1"}}
        end,
        get = function(_, p)
            check(); assert(p == 100)
            return {pane = 100, owner = "hermes-tern", about = "hermes-tern/omp-handoff-review/v1"}
        end,
        set = function(_, p, opts)
            check(); assert(p == 100 and opts.md == nil and opts.view)
            canvasUpdates += 1; table.insert(canvases, opts)
        end,
        open = function(_, opts)
            check(); assert(opts.md == nil and opts.view, "payload must be inert text, not Markdown")
            canvasOpens += 1; table.insert(canvases, opts); return 100
        end,
    }
    local last_request, last_reply
    local tern = {
        plugin = {id = "hermes-tern"},
        command = function(def) assert(not commands[def.id]); commands[def.id] = def end,
        getenv = function(name)
            check()
            if mode == "no_config" then return nil end
            if name == "HERMES_TERN_HANDOFF_BIN" then return "/trusted/hermes-tern" end
            if name == "HERMES_TERN_HANDOFF_STATE" then return "/private/state" end
            error("unexpected environment read")
        end,
        now = function() return clock end,
        timer = function(_, fn)
            table.insert(callbacks, fn)
            return {cancel = function() cancelledTimers += 1 end}
        end,
        ui = {text = function(t) return {k = "text", p = {text = t}} end,
            code = function(t) return {k = "code", p = {text = t}} end,
            col = function(c) return {k = "col", c = c} end},
        json = {
            encode = function(value) last_request = value; return "encoded-request" end,
            decode = function(value) assert(value == "encoded-reply"); return last_reply end,
        },
        process = {run = function(argv, opts, cb)
            check(); launches += 1
            assert(#argv == 5 and argv[1] == "/trusted/hermes-tern" and argv[2] == "omp-handoff"
                and argv[3] == "exchange" and argv[4] == "--state" and argv[5] == "/private/state")
            assert(opts.stdin == "encoded-request" and opts.timeout_ms > 0 and opts.timeout_ms <= 10000)
            local req = last_request
            assert(req.v == 1 and req.request_id == proposal.handoff_id)
            local op = req.op
            if op.type == "arm" then
                assert(op.attempt_id == proposal.attempt_id and op.target_id == proposal.target_id and op.digest == proposal.digest)
            end
            schedule(function(fresh)
                if op.type == "arm" and mode == "lost_arm" then return end
                local grant = not armed and mode ~= "duplicate_arm"
                if op.type == "arm" then armed = true end
                if op.type == "record" then table.insert(records, op.event) end
                local reply = {v = 1, request_id = req.request_id, op = op.type, proposal = proposal,
                    armed = armed and op.type == "prepare", newly_armed = grant, event = op.event, observations = {}}
                if op.type == "arm" and mode == "drift" then surface.surface.id = "s:2" end
                if op.type == "arm" and mode == "stale_callback" then clock = 120001 end
                if mode == "timed_out" then reply.newly_armed = true end
                if mode == "wrong_reply" then reply.request_id = proposal.attempt_id end
                last_reply = reply
                cb({status = mode == "process_error" and 1 or 0, stdout = "encoded-reply", stderr = "", timed_out = mode == "timed_out"}, fresh)
                if mode == "duplicate_callback" or (mode == "duplicate_arm_callback" and op.type == "arm") then
                    last_reply = reply
                    cb({status = 0, stdout = "encoded-reply", stderr = "", timed_out = false}, fresh)
                end
            end)
        end},
    }
    load_plugin(tern)
    assert(launches == 0, "loading must only register")
    assert(commands["omp_handoff"], "missing production handoff command")
    alive = true
    commands.omp_handoff.run(freshContext())
    if mode == "reentry" then commands.omp_handoff.run(freshContext()) end
    alive = false
    local steps = 0
    while #pending > 0 do
        steps += 1; assert(steps < 40, "unbounded callback loop")
        local fn = table.remove(pending, 1)
        alive = true; fn(freshContext()); alive = false
        if mode == "reload" and armed and not reloaded then
            reloaded = true
            pending, callbacks, commands = {}, {}, {}
            load_plugin(tern)
            alive = true; commands.omp_handoff.run(freshContext()); alive = false
        end
    end
    alive = true
    for _, timer in callbacks do clock = 120001; timer(freshContext()) end
    alive = false
    assert(not expiredUse, "a retained context was used")
    local sent = mode == "happy" or mode == "duplicate_callback" or mode == "wait_error" or mode == "ask_error"
        or mode == "reload" or mode == "reentry" or mode == "existing_review" or mode == "duplicate_arm_callback"
    assert(#asks == (sent and 1 or 0), mode .. " unexpected ask count " .. #asks)
    if mode == "happy" or mode == "duplicate_callback" or mode == "duplicate_arm_callback" then
        assert(table.find(records, "ask_returned") and table.find(records, "idle_observed"), "missing observations")
    end
    if mode == "ask_error" or mode == "wait_error" then assert(table.find(records, "outcome_unknown")) end
    if mode == "existing_review" then assert(canvasOpens == 0 and canvasUpdates == 1, "persistent review canvas leaked") end
    if mode == "happy" then assert(cancelledTimers == 1, "completed flow retains pending timer") end
    return launches
end

for _, mode in {"happy", "cancel_prompt", "cancel_target", "cancel_consent", "remote", "waiting", "shell",
    "subagent", "capped", "not_sendable", "draft", "layer", "drift", "duplicate_arm", "lost_arm", "duplicate_callback",
    "wrong_reply", "process_error", "wait_error", "ask_error", "no_config", "reload", "reentry", "stale_callback", "timed_out",
    "existing_review", "capped_layer", "unknown_layer", "retained_main", "main_editor", "duplicate_arm_callback"} do
    exercise(mode, real_proposal)
    print("PASS " .. mode)
end
