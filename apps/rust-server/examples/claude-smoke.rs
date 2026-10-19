//! Opt-in live provider check using generated text; never reads an interview.
use serde_json::json;
use storyfold_core::Sentence;
use storyfold_server::{AppError, Result, topics};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let _ = dotenvy::from_path(root.join(".env"));
    let settings = topics::topic_settings();
    if settings.topic_provider != "Claude" {
        return Err(AppError::bad(
            "Configure Claude before running this opt-in live check.",
        ));
    }
    let areas = [
        "Growing up near the bakery taught me patience.",
        "My mother started baking bread before sunrise.",
        "The neighbors helped repair the bakery after the storm.",
        "My advice is to show up early and help people.",
    ];
    let sentences: Vec<_> = (0..612)
        .map(|i| Sentence {
            id: format!("s{}", i + 1),
            n: i + 1,
            text: format!(
                "{} This is generated interview example {}.",
                areas[i as usize % 4],
                i + 1
            ),
            ..Default::default()
        })
        .collect();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let future = topics::classify_topics(&sentences, &tx, &cancel);
    tokio::pin!(future);
    let start = std::time::Instant::now();
    let result = loop {
        tokio::select! {r=&mut future=>break r?,Some((detail,_))=rx.recv()=>eprintln!("{detail}")}
    };
    assert_eq!(result.assignments.len(), sentences.len());
    assert!((4..=10).contains(&result.topics.len()));
    assert!(sentences.iter().all(|s| {
        result
            .assignments
            .get(&(s.n as u64))
            .is_some_and(|id| result.topics.iter().any(|t| &t.id == id))
    }));
    let report = json!({"passed":true,"runtime":"Rust","provider":"Claude","sentenceCount":sentences.len(),"assignedCount":result.assignments.len(),"topicCount":result.topics.len(),"assignmentBatches":3,"elapsedSeconds":start.elapsed().as_secs_f64(),"input":"Generated text only"});
    let path = root.join("docs/validation/rust-claude.json");
    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}
