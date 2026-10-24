use hermes_tern_gateway::history::{
    HistorySnapshot, MAX_HISTORY_BYTES, MAX_HISTORY_ROWS, PAGE_BYTES, PAGE_ROWS,
};
use serde_json::json;

#[test]
fn pages_preserve_every_projected_row_beyond_live_retention_without_count_conflation() {
    let rows: Vec<_> = (0..300).map(|n| json!({"role":"assistant","row_id":n,"text":format!("row {n}"),"unknown":{"kept":true}})).collect();
    let expected: Vec<_> = rows.iter().map(|row| row.to_string()).collect();
    let history = HistorySnapshot::from_response(json!({"count":420,"messages":rows})).unwrap();
    assert_eq!(history.source_count(), 420);
    assert_eq!(history.row_count(), 300);
    let mut reconstructed = vec![String::new(); history.row_count()];
    for page in 0..history.page_count() {
        let fragments = history.page(page).unwrap();
        assert!(fragments.len() <= PAGE_ROWS);
        assert!(fragments.iter().map(|f| f.text.len()).sum::<usize>() <= PAGE_BYTES);
        for f in fragments {
            assert_eq!(f.start, reconstructed[f.row].len());
            assert_eq!(f.end - f.start, f.text.len());
            reconstructed[f.row].push_str(f.text);
        }
    }
    assert_eq!(reconstructed, expected);
    assert!(history.page(history.page_count()).is_err());
}

#[test]
fn large_unicode_rows_split_losslessly_and_controls_stay_escaped() {
    let row = json!({"role":"tool","name":"same-name","args":{"x":"😀".repeat(PAGE_BYTES)},"content":"\u{1b}]52;bad\u{7}\u{202e}look\nend"});
    let expected = row.to_string().replace('\u{202e}', "\\u202e");
    let history = HistorySnapshot::from_response(json!({"count":1,"messages":[row]})).unwrap();
    assert!(history.page_count() > 1);
    let mut actual = String::new();
    for p in 0..history.page_count() {
        for fragment in history.page(p).unwrap() {
            assert_eq!(fragment.row, 0);
            assert_eq!(fragment.total_bytes, expected.len());
            assert_eq!(fragment.start, actual.len());
            actual.push_str(fragment.text);
        }
    }
    assert_eq!(actual, expected);
    assert!(!actual.chars().any(char::is_control));
    assert!(!actual.contains('\u{202e}'));
}

#[test]
fn bidi_controls_are_escaped_losslessly_in_keys_and_nested_values() {
    let controls = "\u{061c}\u{200e}\u{200f}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}";
    let row = json!({controls: {"text":format!("ع{controls}ב")}});
    let history =
        HistorySnapshot::from_response(json!({"count":1,"messages":[row.clone()]})).unwrap();
    let text = history.page(0).unwrap()[0].text;
    for control in controls.chars() {
        assert!(!text.contains(control));
        assert!(text.contains(&format!("\\u{:04x}", control as u32)));
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).unwrap(),
        row
    );
    assert!(text.contains('ع') && text.contains('ב'));
}

#[test]
fn exact_retention_boundaries_are_inclusive_and_fully_traversable() {
    let row = json!({"text":""});
    let overhead = row.to_string().len();
    let full = HistorySnapshot::from_response(
        json!({"count":1,"messages":[{"text":"x".repeat(MAX_HISTORY_BYTES-overhead)}]}),
    )
    .unwrap();
    let bytes: usize = (0..full.page_count())
        .map(|p| {
            full.page(p)
                .unwrap()
                .iter()
                .map(|f| f.text.len())
                .sum::<usize>()
        })
        .sum();
    assert_eq!(bytes, MAX_HISTORY_BYTES);
    let rows = HistorySnapshot::from_response(
        json!({"count":MAX_HISTORY_ROWS,"messages":vec![json!({});MAX_HISTORY_ROWS]}),
    )
    .unwrap();
    assert_eq!(rows.row_count(), MAX_HISTORY_ROWS);
    let fragments: usize = (0..rows.page_count())
        .map(|p| rows.page(p).unwrap().len())
        .sum();
    assert_eq!(fragments, MAX_HISTORY_ROWS);
}

#[test]
fn empty_history_has_one_empty_page_and_malformed_or_over_limit_refuses() {
    let empty = HistorySnapshot::from_response(json!({"count":0,"messages":[]})).unwrap();
    assert_eq!(empty.page_count(), 1);
    assert!(empty.page(0).unwrap().is_empty());
    for value in [
        json!({}),
        json!({"count":true,"messages":[]}),
        json!({"count":0,"messages":{}}),
        json!({"count":1,"messages":["not a projected row"]}),
    ] {
        assert!(HistorySnapshot::from_response(value).is_err());
    }
    assert!(
        HistorySnapshot::from_response(
            json!({"count":0,"messages":vec![json!({});MAX_HISTORY_ROWS+1]})
        )
        .is_err()
    );
    assert!(
        HistorySnapshot::from_response(
            json!({"count":1,"messages":[{"text":"x".repeat(MAX_HISTORY_BYTES)}]})
        )
        .is_err()
    );
    // Escaped bidi bytes, not just input bytes, count against retained budget.
    assert!(
        HistorySnapshot::from_response(
            json!({"count":1,"messages":[{"text":"\u{202e}".repeat(MAX_HISTORY_BYTES/4)}]})
        )
        .is_err()
    );
}
