use netget::llm::model_selection::{select_best_model, ModelInfo};

#[test]
fn test_select_best_model_by_size() {
    let models = vec![
        ModelInfo {
            name: "small:1b".to_string(),
            size: 1_000_000_000,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        },
        ModelInfo {
            name: "large:30b".to_string(),
            size: 30_000_000_000,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        },
        ModelInfo {
            name: "medium:7b".to_string(),
            size: 7_000_000_000,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        },
    ];

    let best = select_best_model(&models);
    assert_eq!(best, Some("large:30b".to_string()));
}

#[test]
fn test_select_best_model_by_recency() {
    let models = vec![
        ModelInfo {
            name: "old:7b".to_string(),
            size: 7_000_000_000,
            modified_at: "2024-01-01T00:00:00Z".to_string(),
        },
        ModelInfo {
            name: "new:7b".to_string(),
            size: 7_000_000_000,
            modified_at: "2024-12-01T00:00:00Z".to_string(),
        },
    ];

    let best = select_best_model(&models);
    assert_eq!(best, Some("new:7b".to_string()));
}

#[test]
fn test_select_best_model_empty() {
    let models = vec![];
    let best = select_best_model(&models);
    assert_eq!(best, None);
}
