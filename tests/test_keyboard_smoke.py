"""Keyboard/motion autorun contract; real rendering is a separate execution gate."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, requests


class KeyboardSmokeTests(unittest.TestCase):
    def test_keyboard_scenarios_never_use_pointer_to_activate_transcript_or_send(self):
        smoke = module("smoke_native_chat")
        for case in ("keyboard-output", "keyboard-reduced"):
            text, shots = smoke.scenario(case, ["/private/hermes-tern", "chat"], Path("/private/stderr"))
            self.assertEqual(shots, ["selected", "expanded", "refolded"])
            self.assertIn("key Tab", text)
            self.assertIn("key Enter", text)
            self.assertIn("key Space", text)
            self.assertIn("key Escape", text)
            self.assertNotIn('click ".ht-transcript-toggle"', text)
            self.assertNotIn("key Ctrl+d", text)  # exit verified separately via explicit Detach
            smoke.validate_requests(case, requests(case), [])
            with self.assertRaises(ValueError):
                smoke.validate_requests(case, requests(case) + [{"method":"prompt.submit","params":{}}], [])

    def test_readiness_requires_exact_draft_selection_and_motion_state(self):
        smoke = module("smoke_native_chat")
        body = {"ok": True, "count": 1, "nodes": [{"text":" ".join(smoke.LONG_TEXT.split())}]}
        toggle = {"ok": True, "count": 1, "nodes": [{"text":"Collapse retained text", "class":"ht-transcript-toggle ht-keyboard-focus ht-keyboard-ready"}]}
        # Measured Tern 0.9.5 editor tree includes the rendered mode badge.
        editor = {"ok": True, "count": 1, "nodes": [{"text":smoke.KEYBOARD_DRAFT + "inspect"}]}
        buttons = {"ok": True, "count": 1, "nodes": [{"text":"Detach"}]}
        surface = {"ok": True, "count": 1, "nodes": [{"class":"sf-region sf-main sf-still"}]}
        self.assertTrue(smoke.keyboard_ready("keyboard-reduced", "expanded", editor, buttons, body, toggle, surface))
        for slot, key, value in [(0,"text","incompleteinspect"),(0,"text",smoke.KEYBOARD_DRAFT),(1,"text","Send"),(2,"text",body["nodes"][0]["text"][:-1]),(3,"class","ht-transcript-toggle"),(3,"class","ht-transcript-toggle ht-keyboard-focus"),(4,"class","sf-main")]:
            values = copy.deepcopy([editor,buttons,body,toggle,surface])
            values[slot]["nodes"][0][key] = value
            self.assertFalse(smoke.keyboard_ready("keyboard-reduced","expanded",*values))
        plain = copy.deepcopy(surface)
        plain["nodes"][0]["class"] = "sf-main"
        self.assertTrue(smoke.keyboard_ready("keyboard-output","expanded",editor,buttons,body,toggle,plain))
        self.assertFalse(smoke.keyboard_ready("keyboard-output","expanded",editor,buttons,body,toggle,surface))

    def test_motion_setting_requires_exact_preference_choices(self):
        smoke = module("smoke_native_chat")
        good = {"ok": True,"count":3,"nodes":[{"text":"System"},{"text":"On","class":"on"},{"text":"Off"}]}
        self.assertTrue(smoke.motion_choices_ready(good, True))
        self.assertFalse(smoke.motion_choices_ready(good, False))
        for bad in [{"ok":False}, {**good,"count":2}, {**good,"nodes":list(reversed(good["nodes"]))}]:
            self.assertFalse(smoke.motion_choices_ready(bad,True))

    def test_escape_readiness_requires_full_draft_insert_mode_and_editor_focus(self):
        smoke = module("smoke_native_chat")
        editor = {"ok": True, "count": 1, "nodes": [{"text":smoke.KEYBOARD_DRAFT + "insert", "class":"sf-editor focused"}]}
        buttons = {"ok": True, "count": 2, "nodes": [{"text":"Send"}, {"text":"Detach"}]}
        self.assertTrue(smoke.keyboard_composer_ready(editor, buttons))
        for field, value in [("text",smoke.KEYBOARD_DRAFT + "inspect"), ("text",smoke.KEYBOARD_DRAFT[:-1]+"insert"), ("class","sf-editor")]:
            bad = copy.deepcopy(editor)
            bad["nodes"][0][field] = value
            self.assertFalse(smoke.keyboard_composer_ready(bad, buttons))
        self.assertFalse(smoke.keyboard_composer_ready(editor, {"ok":True,"count":1,"nodes":[{"text":"Detach"}]}))


if __name__ == "__main__":
    unittest.main()
