use super::*;

#[test]
fn explicit_location_anchors_follow_canonical_language_variants() {
    let anchors = query_anchors(
        "src/app.mjs:7 types/api.mts#L9 views/index.phtml:4 Gemfile:2 ignored.unknown:8",
    );
    assert_eq!(
        anchors
            .iter()
            .map(|anchor| (anchor.path.as_str(), anchor.line))
            .collect::<Vec<_>>(),
        [
            ("src/app.mjs", Some(7)),
            ("types/api.mts", Some(9)),
            ("views/index.phtml", Some(4)),
            ("Gemfile", Some(2)),
        ]
    );
}

#[test]
fn explicit_location_anchors_keep_supported_auxiliary_files() {
    let anchors = query_anchors("deno.jsonc:3 schema.proto:8 component.vue:12 notes.unknown:4");
    assert_eq!(anchors.len(), 3);
    assert_eq!(anchors[0].path, "deno.jsonc");
    assert_eq!(anchors[1].path, "schema.proto");
    assert_eq!(anchors[2].path, "component.vue");
}
