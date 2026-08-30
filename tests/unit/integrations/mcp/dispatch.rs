use super::*;
use serde_json::json;

#[cfg(test)]
mod agent_context_enrichment_tests {
    use super::*;

    #[test]
    fn worktree_status_warns_on_existing_changes_and_blocks_conflicts() {
        let mut context = json!({
            "targets": [{"path": "src/lib.rs"}],
            "readiness": {"edit": "ready", "next_actions": ["apply_edits", "review_changes", "verify_project"], "advisories": []}
        });
        merge_agent_worktree_status(
            &mut context,
            &json!({
                "available": true,
                "files": [{"path":"src/lib.rs","status":"modified","staged":false,"unstaged":true,"untracked":false}],
                "truncated": false
            }),
        );
        assert_eq!(context["worktree"]["targets"][0]["status"], "modified");
        assert_eq!(context["worktree"]["has_existing_changes"], true);
        assert!(context["readiness"]["advisories"]
            .as_array()
            .unwrap()
            .iter()
            .any(|advisory| advisory == "target_has_worktree_changes"));
        assert_eq!(context["readiness"]["edit"], "ready");

        merge_agent_worktree_status(
            &mut context,
            &json!({
                "available": true,
                "files": [{"path":"src/lib.rs","status":"unmerged","staged":true,"unstaged":true,"untracked":false}],
                "truncated": false
            }),
        );
        assert_eq!(context["readiness"]["edit"], "worktree_conflict");
        assert_eq!(
            context["readiness"]["next_actions"],
            json!(["review_changes"])
        );
    }
}

#[cfg(test)]
mod media_capability_tests {
    use super::*;

    #[test]
    fn media_content_is_fail_closed_without_explicit_client_extension() {
        assert!(!client_supports_media_content(
            &json!({"name":"read_media","arguments":{}}),
            "image",
            "image/png"
        ));
    }

    #[test]
    fn media_content_requires_matching_kind_and_optional_mime_filter() {
        let params = json!({
            "_meta": {
                "io.modelcontextprotocol/clientCapabilities": {
                    "extensions": {
                        (MEDIA_CONTENT_EXTENSION_ID): {
                            "contentTypes": ["image"],
                            "mimeTypes": ["image/png"]
                        }
                    }
                }
            }
        });
        assert!(client_supports_media_content(&params, "image", "image/png"));
        assert!(!client_supports_media_content(
            &params,
            "audio",
            "audio/mpeg"
        ));
        assert!(!client_supports_media_content(
            &params,
            "image",
            "image/jpeg"
        ));
    }
}
