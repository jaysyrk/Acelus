use acelus_rpc::protocol::codes;

const SCHEMA: &str = include_str!("../../../schema/rpc.json");

fn declared_codes() -> serde_json::Map<String, serde_json::Value> {
    let document: serde_json::Value =
        serde_json::from_str(SCHEMA).expect("the rpc schema is valid json");
    document["errors"]["codes"]
        .as_object()
        .expect("the schema declares error codes")
        .clone()
}

#[test]
fn every_code_the_schema_declares_exists_in_the_implementation() {
    let implemented: Vec<(i32, &str)> = vec![
        (codes::INTERNAL, "INTERNAL"),
        (codes::NOT_FOUND, "NOT_FOUND"),
        (codes::ALREADY_EXISTS, "ALREADY_EXISTS"),
        (codes::NETWORK, "NETWORK"),
        (codes::INTEGRITY_FAILED, "INTEGRITY_FAILED"),
        (codes::AUTH_REQUIRED, "AUTH_REQUIRED"),
        (codes::AUTH_EXPIRED, "AUTH_EXPIRED"),
        (codes::XBOX_ACCOUNT_MISSING, "XBOX_ACCOUNT_MISSING"),
        (codes::XBOX_CHILD_ACCOUNT, "XBOX_CHILD_ACCOUNT"),
        (codes::XBOX_REGION_UNAVAILABLE, "XBOX_REGION_UNAVAILABLE"),
        (codes::XBOX_ACCOUNT_BANNED, "XBOX_ACCOUNT_BANNED"),
        (
            codes::XBOX_ADULT_VERIFICATION_REQUIRED,
            "XBOX_ADULT_VERIFICATION_REQUIRED",
        ),
        (codes::ENTITLEMENT_MISSING, "ENTITLEMENT_MISSING"),
        (
            codes::ENTITLEMENT_SIGNATURE_INVALID,
            "ENTITLEMENT_SIGNATURE_INVALID",
        ),
        (codes::AZURE_APP_UNAPPROVED, "AZURE_APP_UNAPPROVED"),
        (codes::JAVA_UNAVAILABLE, "JAVA_UNAVAILABLE"),
        (codes::LOADER_UNSUPPORTED, "LOADER_UNSUPPORTED"),
        (codes::DEPENDENCY_CONFLICT, "DEPENDENCY_CONFLICT"),
        (codes::INSTANCE_RUNNING, "INSTANCE_RUNNING"),
        (codes::SANDBOX_UNAVAILABLE, "SANDBOX_UNAVAILABLE"),
    ];

    let declared = declared_codes();

    for (code, name) in &implemented {
        let key = code.to_string();
        let schema_name = declared
            .get(&key)
            .unwrap_or_else(|| panic!("{name} ({code}) is implemented but not in the schema"));
        assert_eq!(
            schema_name.as_str().unwrap(),
            *name,
            "code {code} is named differently in the schema"
        );
    }

    assert_eq!(
        declared.len(),
        implemented.len(),
        "the schema declares {} codes but {} are implemented; they must not drift",
        declared.len(),
        implemented.len()
    );
}

#[test]
fn the_schema_declares_the_transport_the_implementation_uses() {
    let document: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let framing = document["transport"]["framing"].as_str().unwrap();

    assert!(
        framing.contains("Newline-delimited"),
        "the encoder emits newline delimited frames, so the schema must say so"
    );
}

#[test]
fn every_method_the_schema_declares_is_namespaced() {
    let document: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let methods = document["methods"].as_object().unwrap();

    assert!(!methods.is_empty());
    for name in methods.keys() {
        assert!(
            name.contains('.'),
            "{name} should be namespaced, as every other method is"
        );
    }
}
