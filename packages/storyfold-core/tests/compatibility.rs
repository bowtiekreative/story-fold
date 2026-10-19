use serde_json::{Value, json};
use storyfold_core::*;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/legacy.json")).unwrap()
}
fn project() -> Project {
    serde_json::from_value(fixture()["project"].clone()).unwrap()
}
fn equivalent(a: &Value, b: &Value) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => assert!(
            (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() < 1e-9,
            "{a} != {b}"
        ),
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len());
            for (a, b) in x.iter().zip(y) {
                equivalent(a, b);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            assert_eq!(x.len(), y.len());
            for (k, a) in x {
                equivalent(a, &y[k]);
            }
        }
        _ => assert_eq!(a, b),
    }
}

#[test]
fn typescript_sequence_splitter_and_exports_match() {
    let f = fixture();
    let p = project();
    let sq = sequence(&p);
    equivalent(&serde_json::to_value(&sq).unwrap(), &f["sequence"]);
    let words: Vec<Word> = serde_json::from_value(f["words"].clone()).unwrap();
    equivalent(
        &serde_json::to_value(split_sentences(&words, 4.).unwrap()).unwrap(),
        &f["sentences"],
    );
    assert_eq!(build_srt(&p), f["srt"].as_str().unwrap());
    assert_eq!(build_edl(&p), f["edl"].as_str().unwrap());
    assert_eq!(build_fcpxml(&p), f["fcpxml"].as_str().unwrap());
    let frame = frame_at(&sq, 2.5);
    assert_eq!(frame.audio.unwrap().sentence.id, "s1");
    assert_eq!(frame.video.unwrap().sentence.id, "s2");
    assert_eq!(frame.video_time, 7.5);
    assert!(frame.lcut);
    let opening_parenthesis = [
        Word {
            text: "Not.(".into(),
            start: 0.,
            end: 1.,
        },
        Word {
            text: "complete.".into(),
            start: 1.2,
            end: 2.2,
        },
    ];
    assert_eq!(split_sentences(&opening_parenthesis, 3.).unwrap().len(), 1);
}
#[test]
fn editing_preserves_source_timing_unknown_fields_and_revision() {
    let mut raw = fixture()["project"].clone();
    raw["futureMetadata"] = json!({"preserve":true});
    let p: Project = serde_json::from_value(raw.clone()).unwrap();
    raw["sentences"][0]["start"] = json!(999.);
    raw["sentences"][0]["in"] = json!(1.5);
    raw["cameras"] = json!([{"sourcePath":"/etc/passwd"}]);
    let next = apply_edit(&p, serde_json::from_value(raw).unwrap()).unwrap();
    assert_eq!(next.revision, 1);
    assert_eq!(next.sentences[0].start, 1.);
    assert_eq!(next.sentences[0].r#in, 1.5);
    assert_eq!(next.cameras[0].source_path, "source-A.mp4");
    assert_eq!(next.extra["futureMetadata"], json!({"preserve":true}));
    let edit: ProjectEdit = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
    assert!(
        apply_edit(&next, edit)
            .unwrap_err()
            .contains("another window")
    );
}
#[test]
fn invalid_edits_and_malformed_words_are_rejected() {
    let p = project();
    for patch in [
        json!({"in":-1.}),
        json!({"in":3.9,"out":4.}),
        json!({"topicId":"missing"}),
        json!({"cam":"C"}),
    ] {
        let mut v = serde_json::to_value(&p).unwrap();
        for (k, value) in patch.as_object().unwrap() {
            v["sentences"][0][k] = value.clone();
        }
        assert!(apply_edit(&p, serde_json::from_value(v).unwrap()).is_err());
    }
    for w in [
        Word {
            text: "No.".into(),
            start: 2.,
            end: 1.,
        },
        Word {
            text: "No.".into(),
            start: 0.,
            end: f64::NAN,
        },
        Word {
            text: "Long.".into(),
            start: 0.,
            end: 21.,
        },
        Word {
            text: "Outside.".into(),
            start: 10.,
            end: 11.,
        },
    ] {
        assert!(split_sentences(&[w], 10.).is_err());
    }
    assert!(split_sentences(&[], 10.).is_err());
}
#[test]
fn long_pauses_and_many_lcuts_keep_contiguous_positive_intervals() {
    let words = vec![
        Word {
            text: "First".into(),
            start: 0.,
            end: 1.,
        },
        Word {
            text: "clause,".into(),
            start: 2.,
            end: 4.,
        },
        Word {
            text: "more".into(),
            start: 5.,
            end: 7.,
        },
        Word {
            text: "speech".into(),
            start: 18.,
            end: 19.,
        },
        Word {
            text: "later.".into(),
            start: 100.,
            end: 102.,
        },
    ];
    let sentences = split_sentences(&words, 103.).unwrap();
    assert_eq!(
        sentences.iter().map(|s| s.words.len()).sum::<usize>(),
        words.len()
    );
    assert!(sentences.iter().all(|s| s.end - s.start <= 20.));
    for k in 0..80 {
        let mut p = project();
        p.timeline = vec![TimelineItem::Group {
            topic_id: "one".into(),
        }];
        p.sentences = (0..20)
            .map(|i| Sentence {
                id: format!("s{i}"),
                start: i as f64 * 5.,
                r#in: i as f64 * 5.,
                out: i as f64 * 5. + 0.3 + ((i + k) % 8) as f64 / 2.,
                topic_id: "one".into(),
                lcut: ((i + k) % 26) as f64 / 10.,
                ..Default::default()
            })
            .collect();
        let sq = sequence(&p);
        assert_eq!(sq.clips[0].v_start, 0.);
        for pair in sq.clips.windows(2) {
            assert!(pair[0].v_end > pair[0].v_start);
            assert!((pair[0].v_end - pair[1].v_start).abs() < 1e-9);
            assert_eq!(pair[0].a_end, pair[1].a_start);
        }
        assert_eq!(sq.clips.last().unwrap().v_end, sq.total);
    }
    assert_eq!(tc25(59.999), "00:01:00:00");
    assert_eq!(tc25(3600.04), "01:00:00:01");
    assert_eq!(fmt(59.999), "01:00.0");
}
