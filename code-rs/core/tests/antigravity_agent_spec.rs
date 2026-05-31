use code_core::agent_defaults::agent_model_spec;

#[test]
fn antigravity_replaces_builtin_gemini_agent_path() {
    let spec = agent_model_spec("antigravity").expect("spec present");
    assert_eq!(spec.slug, "antigravity");
    assert_eq!(spec.family, "antigravity");
    assert_eq!(spec.cli, "agy");
    assert!(spec.model_args.is_empty());
    assert!(spec.read_only_args.is_empty());
    assert_eq!(spec.write_args, &["--dangerously-skip-permissions"]);

    assert_eq!(agent_model_spec("agy").expect("alias present").slug, "antigravity");
    assert_eq!(
        agent_model_spec("google-antigravity")
            .expect("alias present")
            .slug,
        "antigravity"
    );
    assert_eq!(
        agent_model_spec("gemini")
            .expect("gemini intent alias present")
            .slug,
        "antigravity"
    );
    assert_eq!(
        agent_model_spec("google")
            .expect("google intent alias present")
            .slug,
        "antigravity"
    );
    assert!(agent_model_spec("gemini-3-flash-preview").is_none());
    assert!(agent_model_spec("gemini-3.1-pro-preview").is_none());
}
