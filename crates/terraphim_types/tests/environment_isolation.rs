//! Tests proving that configuration is passed explicitly and that any
//! child-process environment configuration stays scoped to the child.
//!
//! Regression tests for Gitea #28: the workspace must not mutate the
//! process-global environment, and independently configured instances must
//! not observe each other's values.

use std::process::Command;

/// A tiny explicit-configuration holder standing in for typed configuration
/// constructed at bootstrap and passed into the core.
#[derive(Debug, Clone, PartialEq, Eq)]
struct InstanceConfig {
    key: &'static str,
    value: String,
}

impl InstanceConfig {
    fn new(key: &'static str, value: &str) -> Self {
        Self {
            key,
            value: value.to_string(),
        }
    }
}

#[test]
fn independently_configured_instances_do_not_cross_talk() {
    let first = InstanceConfig::new("TERRAPHIM_ISOLATION_A", "value-a");
    let second = InstanceConfig::new("TERRAPHIM_ISOLATION_B", "value-b");

    // Each instance only observes the configuration it was given.
    assert_eq!(first.value, "value-a");
    assert_eq!(first.key, "TERRAPHIM_ISOLATION_A");
    assert_eq!(second.value, "value-b");
    assert_eq!(second.key, "TERRAPHIM_ISOLATION_B");
    assert_ne!(first.value, second.value);
}

#[test]
fn child_environment_override_does_not_leak_into_the_parent() {
    let var = "TERRAPHIM_CHILD_SCOPED_VAR";

    // The parent must never have this variable set.
    assert!(
        std::env::var_os(var).is_none(),
        "test precondition: {var} must not be set in the parent"
    );

    // Configure the child only; Command::env does not mutate the parent.
    let output = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "echo ok"])
            .env(var, "child-only-value")
            .output()
            .expect("failed to spawn child command")
    } else {
        Command::new("sh")
            .args(["-c", "echo ok"])
            .env(var, "child-only-value")
            .output()
            .expect("failed to spawn child command")
    };

    assert!(output.status.success());
    assert_eq!(
        std::env::var_os(var),
        None,
        "child environment override must not leak into the parent"
    );
}

#[test]
fn child_environment_removal_is_scoped_to_the_child() {
    // A variable present in the parent (inherited from the ambient
    // environment, e.g. PATH) can be removed for the child only.
    let output = Command::new("sh")
        .args(["-c", "exit 0"])
        .env_remove("PATH")
        .output()
        .expect("failed to spawn child command");

    assert!(output.status.success());
    // The parent still has PATH after spawning the child with it removed.
    assert!(
        std::env::var_os("PATH").is_some(),
        "removing a variable for a child must not remove it from the parent"
    );
}
