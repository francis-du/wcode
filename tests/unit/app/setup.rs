use super::*;
use std::io::Cursor;

#[test]
fn setup_scope_retries_invalid_choices_and_eof_never_means_consent() {
    for (input, expected) in [
        ("\n", SetupScope::Global),
        ("2\n", SetupScope::Project),
        ("mistyped\n2\n", SetupScope::Project),
    ] {
        let mut output = Vec::new();
        assert_eq!(
            choose_scope_with_io(&mut Cursor::new(input), &mut output).unwrap(),
            expected
        );
        if input.starts_with("mistyped") {
            assert!(String::from_utf8(output)
                .unwrap()
                .contains("Choose 1, 2 or 3"));
        }
    }
    for input in ["", "mistyped\n", "3\n", "q\n", "Q\n"] {
        assert!(choose_scope_with_io(&mut Cursor::new(input), &mut Vec::new()).is_err());
    }
}
