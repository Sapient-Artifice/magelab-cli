use magelab_cli::repl::approval::ApprovalPolicy;

#[test]
fn test_auto_approve_safe_tools() {
    let policy = ApprovalPolicy::new(vec!["read_file".into(), "search_files".into()], false);
    assert!(policy.is_auto_approved("read_file"));
    assert!(policy.is_auto_approved("search_files"));
    assert!(!policy.is_auto_approved("bash_commands"));
    assert!(!policy.is_auto_approved("write_file"));
}

#[test]
fn test_yolo_approves_everything() {
    let policy = ApprovalPolicy::new(vec![], true);
    assert!(policy.is_auto_approved("bash_commands"));
    assert!(policy.is_auto_approved("write_file"));
    assert!(policy.is_auto_approved("anything_at_all"));
}
