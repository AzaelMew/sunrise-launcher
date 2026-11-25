import unittest

from tools.ci import lang_keys_check as check


class KeysIn(unittest.TestCase):
    def test_reads_plain_multiline_and_raw_literals(self):
        text = '''let a = tr("Hello");
        let b = trf(
            "Took {minutes} min",
            &[("minutes", "2")],
        );
        let c = tr(r#"raw "quotes""#);'''
        self.assertEqual(
            list(check.keys_in(text)),
            [("Hello", 1), ("Took {minutes} min", 3), ('raw "quotes"', 6)],
        )

    def test_decodes_rust_escapes(self):
        text = r'let a = tr("one \"quoted\" line\nand {name}");'
        self.assertEqual(
            list(check.keys_in(text)), [('one "quoted" line\nand {name}', 1)]
        )

    def test_ignores_expressions_and_constants(self):
        text = 'tr(expr); tr(NAME); trf(format!("{x}")); tr();'
        self.assertEqual(list(check.keys_in(text)), [])

    def test_a_continued_literal_drops_the_indentation_and_the_break(self):
        text = 'tr("First, \\\n                second.")'
        self.assertEqual(
            list(check.keys_in(text)), [("First, second.", 1)]
        )

    def test_reads_the_second_argument_of_tr_in(self):
        text = 'tr_in(Language::French, "Save"); tr_in(other(language, 1), "Open")'
        self.assertEqual(list(check.keys_in(text)), [("Save", 1), ("Open", 1)])

    def test_a_call_inside_a_string_or_comment_is_not_a_call(self):
        text = 'let fixture = "tr(\"not a key\")";\n// tr("nor this")\nlet real = tr("Real");'
        self.assertEqual(list(check.keys_in(text)), [("Real", 3)])

    def test_a_closing_parenthesis_in_the_text_does_not_end_the_literal(self):
        self.assertEqual(list(check.keys_in('tr("a ) b")')), [("a ) b", 1)])

    def test_missing_keys_reports_line_and_text(self):
        catalog = {"Known": "Connu"}
        keys = [("Known", 1), ("Unknown", 7)]
        self.assertEqual(check.missing_keys(keys, catalog), [("Unknown", 7)])


if __name__ == "__main__":
    unittest.main()
