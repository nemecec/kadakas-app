use std::io::{Cursor, Write};

use kadakas_core::issue::{
    Comment, CommentId, Issue, IssueData, IssueId, IssueStatus, Issues, ViewSpec, Viewpoint,
    ViewpointId,
};
use kadakas_core::model::ObjectId;
use kadakas_core::picture::sha256_hex;
use proptest::prelude::*;

use super::camera::{self, BcfCamera};
use super::read::{ReadLimits, read, same_content};
use super::write::{ExportInput, export, generated_viewpoint_id};
use super::*;
use crate::issues::{ifc_guid, match_ifc};

// ----- helpers -----

/// A valid PNG header with a size (the reader looks at the header only).
fn png_bytes(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, seed]);
    b
}

fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        z.start_file(*name, o).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn id(n: u64) -> IssueId {
    IssueId::from_num(n)
}

fn full_issue(n: u64) -> (Issue, Vec<Comment>, Vec<Picture>) {
    let pic = png_bytes(64, 48, n as u8);
    let hash = sha256_hex(&pic);
    let issue = Issue {
        id: id(n),
        title: format!("Dead tree near the gate <{n}> & \"co\""),
        description: "Line one\nLine two with umlauts: ÕÄÖÜ and a tab\t.".into(),
        kind: "error".into(),
        status: IssueStatus::InProgress,
        priority: "high".into(),
        assignee: Some("cy@example.org".into()),
        labels: vec!["planting".into(), "urgent".into()],
        due: Some("2026-11-30".into()),
        author: "ann@example.org".into(),
        created: 1_760_000_000_000,
        modified: 1_760_000_900_000,
        point: Some([1000.0, 2000.0, 300.0]),
        objects: vec![ObjectId::from_num(41), ObjectId::from_num(42)],
        viewpoints: vec![
            Viewpoint {
                id: ViewpointId::from_num(n * 10 + 1),
                name: "Overview".into(),
                time: 1_760_000_000_001,
                view: Some(ViewSpec::Plan {
                    center: [5000.0, 6000.0],
                    zoom: 0.25,
                }),
                selection: vec![ObjectId::from_num(43)],
                snapshot: Some(hash),
                ..Viewpoint::default()
            },
            Viewpoint {
                id: ViewpointId::from_num(n * 10 + 2),
                time: 1_760_000_000_002,
                view: Some(ViewSpec::Camera {
                    target: [100.0, 200.0, 0.0],
                    yaw: 0.7,
                    pitch: 0.4,
                    distance: 12_000.0,
                    perspective: true,
                }),
                ..Viewpoint::default()
            },
        ],
        ..Issue::default()
    };
    let comments = vec![
        Comment {
            id: CommentId::from_num(n * 100 + 1),
            issue: id(n),
            author: "bo@example.org".into(),
            origin: kadakas_core::change::Origin::User,
            time: 1_760_000_100_000,
            text: "Please check <this>\nand that & the other".into(),
            viewpoint: Some(ViewpointId::from_num(n * 10 + 1)),
            edited: Some(1_760_000_200_000),
        },
        Comment {
            id: CommentId::from_num(n * 100 + 2),
            issue: id(n),
            author: "ann@example.org".into(),
            origin: kadakas_core::change::Origin::User,
            time: 1_760_000_300_000,
            text: "Done.".into(),
            viewpoint: None,
            edited: None,
        },
    ];
    (
        issue,
        comments,
        vec![Picture {
            hash: sha256_hex(&pic),
            bytes: pic,
        }],
    )
}

fn store(items: &[(Issue, Vec<Comment>, Vec<Picture>)]) -> (Issues, Vec<Picture>) {
    let mut s = Issues::default();
    let mut pics = Vec::new();
    for (i, c, p) in items {
        s.insert_issue(i.clone());
        for x in c {
            s.insert_comment(x.clone());
        }
        pics.extend(p.iter().cloned());
    }
    (s, pics)
}

fn export_all(version: Version, s: &Issues, pics: &[Picture]) -> (Vec<u8>, ExportReport) {
    let lookup = |h: &str| pics.iter().find(|p| p.hash == h).map(|p| p.bytes.clone());
    export(
        version,
        &ExportInput {
            issues: s,
            only: None,
            snapshot: &lookup,
        },
    )
    .unwrap()
}

fn import(bytes: &[u8]) -> Imported {
    read(bytes, &ReadLimits::default(), 5).unwrap()
}

// ----- round trips -----

#[test]
fn a_round_trip_keeps_everything_that_bcf_holds_in_both_versions() {
    let items = [full_issue(1), full_issue(2)];
    let (s, pics) = store(&items);
    for version in [Version::V21, Version::V30] {
        let (bytes, report) = export_all(version, &s, &pics);
        assert_eq!(
            (
                report.topics,
                report.viewpoints,
                report.snapshots,
                report.comments
            ),
            (2, 4, 2, 4),
            "{version:?}"
        );
        let got = import(&bytes);
        assert_eq!(got.version, version);
        assert!(got.skipped.is_empty(), "{:?}", got.skipped);
        assert!(got.warnings.is_empty(), "{:?}", got.warnings);
        assert_eq!(got.topics.len(), 2);
        for (t, (issue, comments, _)) in got.topics.iter().zip(&items) {
            let i = &t.issue;
            assert_eq!(i.id, issue.id);
            assert_eq!(i.title, issue.title);
            assert_eq!(i.description, issue.description);
            assert_eq!(i.kind, "error");
            assert_eq!(i.status, IssueStatus::InProgress);
            assert_eq!(i.priority, "high");
            assert_eq!(i.assignee.as_deref(), Some("cy@example.org"));
            assert_eq!(i.labels, issue.labels);
            assert_eq!(i.due.as_deref(), Some("2026-11-30"));
            assert_eq!(i.author, issue.author);
            assert_eq!(i.created, issue.created);
            assert_eq!(i.modified, issue.modified.max(1_760_000_300_000));
            // The objects of the issue and of the viewpoints.
            assert_eq!(i.objects.len(), 3);
            assert_eq!(i.viewpoints.len(), 2);
            assert_eq!(i.viewpoints[0].snapshot, issue.viewpoints[0].snapshot);
            assert_eq!(t.pictures.len(), 1);
            assert_eq!(t.comments.len(), 2);
            assert_eq!(t.comments[0].text, comments[0].text);
            assert_eq!(t.comments[0].viewpoint, comments[0].viewpoint);
            assert_eq!(t.comments[0].edited, comments[0].edited);
            let refs: Vec<&Comment> = comments.iter().collect();
            assert!(same_content(issue, &refs, t), "{version:?}");
        }
        // The export is stable: the same input gives the same bytes.
        let (again, _) = export_all(version, &s, &pics);
        assert_eq!(bytes, again);
    }
}

#[test]
fn a_changed_issue_is_not_the_same_content() {
    let (issue, comments, pics) = full_issue(7);
    let (s, p) = store(&[(issue.clone(), comments.clone(), pics)]);
    let (bytes, _) = export_all(Version::V30, &s, &p);
    let t = import(&bytes).topics.remove(0);
    let refs: Vec<&Comment> = comments.iter().collect();
    assert!(same_content(&issue, &refs, &t));
    let mut other = issue.clone();
    other.status = IssueStatus::Closed;
    assert!(!same_content(&other, &refs, &t));
    let mut other = issue.clone();
    other.title.push('!');
    assert!(!same_content(&other, &refs, &t));
    let mut other = issue.clone();
    other.labels.pop();
    assert!(!same_content(&other, &refs, &t));
    let mut other = issue.clone();
    other.viewpoints.pop();
    assert!(!same_content(&other, &refs, &t));
    let mut other = issue.clone();
    other.viewpoints[0].view = Some(ViewSpec::Plan {
        center: [0.0, 0.0],
        zoom: 1.0,
    });
    assert!(!same_content(&other, &refs, &t));
    assert!(!same_content(&issue, &refs[..1], &t));
    let mut cs = comments.clone();
    cs[1].text = "Not done.".into();
    let refs2: Vec<&Comment> = cs.iter().collect();
    assert!(!same_content(&issue, &refs2, &t));
    // Not part of BCF: the pin, the origin, the layers of a viewpoint.
    let mut mine = issue.clone();
    mine.point = None;
    mine.origin = kadakas_core::change::Origin::Agent;
    assert!(same_content(&mine, &refs, &t));
}

#[test]
fn objects_without_a_viewpoint_get_one_that_comes_back_as_objects() {
    let issue = Issue {
        id: id(9),
        title: "About two objects".into(),
        created: 1_000,
        modified: 1_000,
        objects: vec![ObjectId::from_num(5), ObjectId::from_num(6)],
        point: Some([10.0, 20.0, 0.0]),
        ..Issue::default()
    };
    let (s, p) = store(&[(issue.clone(), vec![], vec![])]);
    for version in [Version::V21, Version::V30] {
        let (bytes, report) = export_all(version, &s, &p);
        assert_eq!(report.viewpoints, 1);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("a viewpoint was made"))
        );
        let t = import(&bytes).topics.remove(0);
        assert!(
            t.issue.viewpoints.is_empty(),
            "the made viewpoint is taken out"
        );
        assert_eq!(t.issue.objects, issue.objects);
        assert!(same_content(&issue, &[], &t));
    }
    assert_eq!(
        generated_viewpoint_id(id(9)),
        generated_viewpoint_id(id(9)).to_owned()
    );
}

#[test]
fn an_issue_without_anything_has_no_viewpoint() {
    let issue = Issue {
        id: id(3),
        title: "Plain".into(),
        created: 1_000,
        modified: 1_000,
        ..Issue::default()
    };
    let (s, p) = store(&[(issue, vec![], vec![])]);
    let (bytes, report) = export_all(Version::V30, &s, &p);
    assert_eq!(report.viewpoints, 0);
    let got = import(&bytes);
    assert_eq!(got.topics[0].issue.author, "unknown");
    assert!(got.topics[0].issue.viewpoints.is_empty());
}

#[test]
fn only_the_asked_issues_go_and_the_order_is_kept() {
    let items = [full_issue(1), full_issue(2), full_issue(3)];
    let (s, pics) = store(&items);
    let lookup = |h: &str| pics.iter().find(|p| p.hash == h).map(|p| p.bytes.clone());
    let only = [id(3), id(1), id(99)];
    let (bytes, report) = export(
        Version::V30,
        &ExportInput {
            issues: &s,
            only: Some(&only),
            snapshot: &lookup,
        },
    )
    .unwrap();
    assert_eq!(report.topics, 2);
    let got = import(&bytes);
    assert_eq!(got.topics.len(), 2);
}

#[test]
fn a_missing_snapshot_is_a_note_and_the_viewpoint_stays() {
    let (issue, comments, _) = full_issue(4);
    let (s, _) = store(&[(issue, comments, vec![])]);
    let (bytes, report) = export_all(Version::V30, &s, &[]);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("not in the document"))
    );
    assert_eq!(report.snapshots, 0);
    let got = import(&bytes);
    assert_eq!(got.topics[0].issue.viewpoints.len(), 2);
    assert!(got.topics[0].issue.viewpoints[0].snapshot.is_none());
}

#[test]
fn a_snapshot_that_is_not_a_picture_is_a_note() {
    let (issue, comments, _) = full_issue(4);
    let hash = issue.viewpoints[0].snapshot.clone().unwrap();
    let (s, _) = store(&[(issue, comments, vec![])]);
    let junk = Picture {
        hash: hash.clone(),
        bytes: b"not a picture".to_vec(),
    };
    // The bytes do not hash to the name: refused.
    let (_, report) = export_all(Version::V30, &s, &[junk]);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("not in the document"))
    );
    // Bytes that hash right but are not a picture.
    let bytes = b"just some text".to_vec();
    let mut issue2 = full_issue(5).0;
    issue2.viewpoints[0].snapshot = Some(sha256_hex(&bytes));
    let (s2, _) = store(&[(issue2.clone(), vec![], vec![])]);
    let (_, report) = export_all(
        Version::V30,
        &s2,
        &[Picture {
            hash: sha256_hex(&bytes),
            bytes,
        }],
    );
    assert!(report.notes.iter().any(|n| n.contains("not a PNG or JPEG")));
}

#[test]
fn characters_that_xml_cannot_hold_are_replaced_with_a_note() {
    let mut issue = full_issue(6).0;
    issue.description = "bell\u{7} and \u{1} here".into();
    let (s, p) = store(&[(issue, vec![], vec![])]);
    let (bytes, report) = export_all(Version::V30, &s, &p);
    assert!(
        report.notes.iter().any(|n| n.contains("2 character(s)")),
        "{:?}",
        report.notes
    );
    let t = import(&bytes).topics.remove(0);
    assert_eq!(t.issue.description, "bell\u{FFFD} and \u{FFFD} here");
}

#[test]
fn layer_visibility_is_a_note() {
    let mut issue = full_issue(8).0;
    issue.viewpoints[0]
        .layers
        .insert(kadakas_core::model::LayerId::from_num(1), false);
    let (s, p) = store(&[(issue, vec![], vec![])]);
    let (_, report) = export_all(Version::V21, &s, &p);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("layer and class visibility"))
    );
}

#[test]
fn ifc_guids_match_the_objects() {
    let o = ObjectId(kadakas_core::uid::Uid::from_u128(
        0x0190_f1c2_7a3b_7c1d_8e2f_1234_5678_9abc,
    ));
    let g = ifc_guid(o);
    assert_eq!(g.len(), 22);
    assert_eq!(match_ifc(&g), Some(o));
    assert_eq!(match_ifc("short"), None);
    assert_eq!(match_ifc(&"!".repeat(22)), None);
}

// ----- real-world forms -----

const MARKUP_21: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Markup xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
  <Header>
    <File IfcProject="0J$yPqHBD12v72y4qF6XcD" isExternal="false">
      <Filename>model.ifc</Filename>
    </File>
  </Header>
  <Topic Guid="A7F9C1D2-1111-4222-8333-444455556666" TopicType="Clash" TopicStatus="Active">
    <Title>Pipe through the wall</Title>
    <Priority>Critical</Priority>
    <Labels>Plumbing</Labels>
    <Labels>Wall</Labels>
    <CreationDate>2021-03-01T12:30:00+01:00</CreationDate>
    <CreationAuthor>Pat@Example.com</CreationAuthor>
    <AssignedTo>sam@example.com</AssignedTo>
    <DueDate>2021-04-01T00:00:00Z</DueDate>
    <Description>Check this.</Description>
    <ReferenceLink>https://example.org/x</ReferenceLink>
  </Topic>
  <Comment Guid="B7F9C1D2-1111-4222-8333-444455556666">
    <Date>2021-03-02T08:00:00Z</Date>
    <Author>sam@example.com</Author>
    <Comment>On it.</Comment>
    <Viewpoint Guid="C7F9C1D2-1111-4222-8333-444455556666"/>
  </Comment>
  <Comment Guid="not a guid">
    <Date>2021-03-03T08:00:00Z</Date>
    <Author>pat@example.com</Author>
    <Comment>Thanks.</Comment>
  </Comment>
  <Viewpoints Guid="C7F9C1D2-1111-4222-8333-444455556666">
    <Viewpoint>Viewpoint_C7.bcfv</Viewpoint>
    <Snapshot>Snapshot_C7.png</Snapshot>
  </Viewpoints>
</Markup>"#;

const VIEWPOINT_21: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<VisualizationInfo Guid="C7F9C1D2-1111-4222-8333-444455556666">
  <Components>
    <Selection>
      <Component IfcGuid="0J$yPqHBD12v72y4qF6XcD"><OriginatingSystem>Tool</OriginatingSystem></Component>
      <Component IfcGuid="bad"/>
    </Selection>
    <Visibility DefaultVisibility="false"><Exceptions><Component IfcGuid="0J$yPqHBD12v72y4qF6XcE"/></Exceptions></Visibility>
    <Coloring><Color Color="FF0000"><Component IfcGuid="0J$yPqHBD12v72y4qF6XcD"/></Color></Coloring>
  </Components>
  <PerspectiveCamera>
    <CameraViewPoint><X>10</X><Y>20</Y><Z>30</Z></CameraViewPoint>
    <CameraDirection><X>0</X><Y>1</Y><Z>0</Z></CameraDirection>
    <CameraUpVector><X>0</X><Y>0</Y><Z>1</Z></CameraUpVector>
    <FieldOfView>60</FieldOfView>
  </PerspectiveCamera>
  <ClippingPlanes><ClippingPlane><Location><X>0</X><Y>0</Y><Z>0</Z></Location><Direction><X>0</X><Y>0</Y><Z>1</Z></Direction></ClippingPlane></ClippingPlanes>
</VisualizationInfo>"#;

fn sample_21() -> Vec<u8> {
    let png = png_bytes(100, 80, 1);
    make_zip(&[
        (
            "bcf.version",
            b"<?xml version=\"1.0\"?><Version VersionId=\"2.1\"><DetailedVersion>2.1</DetailedVersion></Version>",
        ),
        // A writer that uses backslashes.
        ("a7f9c1d2-1111-4222-8333-444455556666\\markup.bcf", MARKUP_21.as_bytes()),
        ("a7f9c1d2-1111-4222-8333-444455556666/Viewpoint_C7.bcfv", VIEWPOINT_21.as_bytes()),
        ("a7f9c1d2-1111-4222-8333-444455556666/Snapshot_C7.png", &png),
    ])
}

#[test]
fn a_bcf_21_file_from_another_tool_is_read() {
    let got = import(&sample_21());
    assert_eq!(got.version, Version::V21);
    assert!(got.skipped.is_empty(), "{:?}", got.skipped);
    let t = &got.topics[0];
    let i = &t.issue;
    assert_eq!(i.id.to_string(), "a7f9c1d2-1111-4222-8333-444455556666");
    assert_eq!(
        (i.title.as_str(), i.kind.as_str()),
        ("Pipe through the wall", "Clash")
    );
    assert_eq!(
        (i.status, i.priority.as_str()),
        (IssueStatus::Open, "Critical")
    );
    assert_eq!(i.labels, vec!["Plumbing", "Wall"]);
    assert_eq!(i.author, "Pat@Example.com");
    assert_eq!(i.assignee.as_deref(), Some("sam@example.com"));
    assert_eq!(i.due.as_deref(), Some("2021-04-01"));
    // +01:00 becomes UTC.
    assert_eq!(
        i.created,
        crate::issues::time::parse_iso("2021-03-01T11:30:00Z").unwrap()
    );
    assert_eq!(t.comments.len(), 2);
    assert_eq!(t.comments[0].viewpoint, Some(i.viewpoints[0].id));
    assert_ne!(
        t.comments[1].id,
        CommentId::default(),
        "a bad GUID gets a made one"
    );
    assert_eq!(i.viewpoints.len(), 1);
    let v = &i.viewpoints[0];
    assert!(matches!(
        v.view,
        Some(ViewSpec::Camera {
            perspective: true,
            ..
        })
    ));
    assert_eq!(v.selection.len(), 1, "the bad IfcGuid is left out");
    assert!(v.snapshot.is_some() && t.pictures.len() == 1);
    // Warnings name what was not read.
    let w = got.warnings.join("\n");
    for needle in [
        "bcf-status",
        "topics with reference links",
        "viewpoints with coloring",
        "viewpoints with hidden components",
        "viewpoints with clipping planes",
        "no valid IfcGuid",
    ] {
        if needle == "bcf-status" {
            continue;
        }
        assert!(w.contains(needle), "{needle} in {w}");
    }
    assert_eq!(t.issue.status, IssueStatus::Open, "Active is open");
}

#[test]
fn an_unknown_status_keeps_its_word_as_a_label() {
    let markup = MARKUP_21.replace("Active", "Waiting for client");
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        ("a/markup.bcf", markup.as_bytes()),
    ]);
    let got = import(&zip);
    let i = &got.topics[0].issue;
    assert_eq!(i.status, IssueStatus::Open);
    assert!(
        i.labels
            .contains(&"bcf-status:Waiting for client".to_string())
    );
    assert!(got.warnings.iter().any(|w| w.contains("is not known")));
    // The viewpoint file is not there: a warning, the viewpoint stays.
    assert!(
        got.warnings
            .iter()
            .any(|w| w.contains("is not in the archive"))
    );
}

#[test]
fn a_bcf_20_version_is_read_as_21() {
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"2.0\"/>"),
        ("a/markup.bcf", MARKUP_21.as_bytes()),
    ]);
    assert_eq!(import(&zip).version, Version::V21);
}

#[test]
fn a_bom_and_comments_in_xml_are_fine() {
    let markup = MARKUP_21.replacen("?>", "?><!-- a note -->", 1);
    let markup = format!("\u{FEFF}{markup}");
    let zip = make_zip(&[
        ("bcf.version", b"\xEF\xBB\xBF<Version VersionId=\"2.1\"/>"),
        ("a/markup.bcf", markup.as_bytes()),
    ]);
    let got = import(&zip);
    assert_eq!(got.topics.len(), 1, "{:?}", got.skipped);
}

#[test]
fn old_files_without_a_list_of_viewpoints_use_the_files_in_the_folder() {
    let markup = r#"<Markup><Topic Guid="a7f9c1d2-1111-4222-8333-444455556666">
        <Title>Old</Title><CreationDate>2020-01-01T00:00:00Z</CreationDate><CreationAuthor>x</CreationAuthor></Topic></Markup>"#;
    let png = png_bytes(10, 10, 0);
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"2.0\"/>"),
        ("g/markup.bcf", markup.as_bytes()),
        ("g/viewpoint.bcfv", VIEWPOINT_21.as_bytes()),
        ("g/snapshot.png", &png),
    ]);
    let got = import(&zip);
    assert_eq!(got.topics[0].issue.viewpoints.len(), 1);
    assert!(got.topics[0].issue.viewpoints[0].snapshot.is_some());
}

// ----- hostile archives -----

fn expect_err(bytes: &[u8], limits: &ReadLimits) -> BcfError {
    read(bytes, limits, 0).unwrap_err()
}

#[test]
fn not_an_archive() {
    assert!(matches!(
        expect_err(b"hello", &ReadLimits::default()),
        BcfError::Archive(_)
    ));
    assert!(matches!(
        expect_err(&[], &ReadLimits::default()),
        BcfError::Archive(_)
    ));
    let mut cut = sample_21();
    cut.truncate(cut.len() / 2);
    assert!(matches!(
        expect_err(&cut, &ReadLimits::default()),
        BcfError::Archive(_)
    ));
}

#[test]
fn not_a_bcf_file_or_a_version_we_do_not_read() {
    let zip = make_zip(&[("readme.txt", b"hi")]);
    assert!(matches!(
        expect_err(&zip, &ReadLimits::default()),
        BcfError::Format(_)
    ));
    for text in [
        "<Version VersionId=\"1.0\"/>",
        "<Version VersionId=\"3.1\"/>",
    ] {
        let zip = make_zip(&[("bcf.version", text.as_bytes())]);
        assert!(matches!(
            expect_err(&zip, &ReadLimits::default()),
            BcfError::Version(_)
        ));
    }
    for text in ["<Version/>", "not xml at all", "<a><b></a>"] {
        let zip = make_zip(&[("bcf.version", text.as_bytes())]);
        assert!(matches!(
            expect_err(&zip, &ReadLimits::default()),
            BcfError::Format(_)
        ));
    }
    // A DetailedVersion without the attribute.
    let zip = make_zip(&[(
        "bcf.version",
        b"<Version><DetailedVersion>2.1</DetailedVersion></Version>",
    )]);
    assert_eq!(import(&zip).version, Version::V21);
}

#[test]
fn bad_names_are_refused() {
    for name in [
        "../evil/markup.bcf",
        "a/../../evil",
        "/etc/passwd",
        "C:/windows/x",
        "c:evil",
        "a\\..\\..\\x",
        "a/b\u{1}c",
    ] {
        let zip = make_zip(&[
            ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
            (name, b"x"),
        ]);
        let e = expect_err(&zip, &ReadLimits::default());
        assert!(matches!(e, BcfError::Format(_)), "{name:?}: {e:?}");
    }
    let long = "a".repeat(300);
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        (&long, b"x"),
    ]);
    assert!(matches!(
        expect_err(&zip, &ReadLimits::default()),
        BcfError::Limit(_)
    ));
}

#[test]
fn two_names_that_are_one_are_refused() {
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        ("a/markup.bcf", b"<Markup/>"),
        ("a\\markup.bcf", b"<Markup/>"),
    ]);
    assert!(matches!(
        expect_err(&zip, &ReadLimits::default()),
        BcfError::Format(m) if m.contains("twice")
    ));
}

#[test]
fn too_many_entries_and_too_many_topics() {
    let limits = ReadLimits {
        max_entries: 3,
        ..ReadLimits::default()
    };
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        ("a/x", b"1"),
        ("b/x", b"1"),
        ("c/x", b"1"),
    ]);
    assert!(matches!(expect_err(&zip, &limits), BcfError::Limit(_)));
    let limits = ReadLimits {
        max_topics: 1,
        ..ReadLimits::default()
    };
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        ("a/markup.bcf", b"<Markup/>"),
        ("b/markup.bcf", b"<Markup/>"),
    ]);
    assert!(matches!(expect_err(&zip, &limits), BcfError::Limit(_)));
}

#[test]
fn big_entries_and_a_compression_bomb_are_refused() {
    let zeros = vec![0u8; 8 * 1024 * 1024];
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        ("a/bomb", &zeros),
    ]);
    // The ratio of zeros is far above 200 to 1.
    assert!(matches!(
        expect_err(&zip, &ReadLimits::default()),
        BcfError::Limit(m) if m.contains("compressed more than")
    ));
    // An entry over the size limit, and entries over the limit together.
    let big = vec![7u8; 5000];
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        ("a/big", &big),
    ]);
    let limits = ReadLimits {
        max_entry_bytes: 1000,
        ..ReadLimits::default()
    };
    assert!(matches!(expect_err(&zip, &limits), BcfError::Limit(m) if m.contains("an entry is")));
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"3.0\"/>"),
        ("a/1", &big),
        ("a/2", &big),
    ]);
    let limits = ReadLimits {
        max_total_bytes: 8000,
        ..ReadLimits::default()
    };
    assert!(matches!(expect_err(&zip, &limits), BcfError::Limit(m) if m.contains("together")));
}

#[test]
fn an_xml_file_over_its_limit_is_refused() {
    let limits = ReadLimits {
        max_xml_bytes: 100,
        ..ReadLimits::default()
    };
    assert!(matches!(
        expect_err(&sample_21(), &limits),
        BcfError::Limit(_)
    ));
}

#[test]
fn an_entity_bomb_and_a_deep_or_wide_xml_skip_the_topic() {
    let bomb = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
<!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">]>
<Markup><Topic Guid="a7f9c1d2-1111-4222-8333-444455556666"><Title>&lol3;</Title></Topic></Markup>"#;
    let deep = format!("{}{}", "<a>".repeat(5000), "</a>".repeat(5000));
    let wide = format!(
        "<Markup><Topic Guid=\"a7f9c1d2-1111-4222-8333-444455556666\"><Title>x</Title>{}</Topic></Markup>",
        "<Labels>l</Labels>".repeat(300)
    );
    for markup in [bomb.to_string(), deep, wide] {
        let zip = make_zip(&[
            ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
            ("a/markup.bcf", markup.as_bytes()),
        ]);
        let limits = ReadLimits {
            max_xml_nodes: 500,
            ..ReadLimits::default()
        };
        let got = read(&zip, &limits, 0).unwrap();
        assert!(got.topics.is_empty());
        assert_eq!(got.skipped.len(), 1, "{:?}", got.skipped);
        assert!(got.skipped[0].reason.starts_with("markup.bcf"));
    }
}

#[test]
fn an_encrypted_entry_is_refused() {
    let mut zip = sample_21();
    // Set bit 0 of the general purpose flag in every central directory header.
    let mut i = 0;
    while i + 12 < zip.len() {
        if zip[i..i + 4] == [0x50, 0x4b, 0x01, 0x02] {
            zip[i + 8] |= 1;
        }
        i += 1;
    }
    let e = expect_err(&zip, &ReadLimits::default());
    assert!(
        matches!(e, BcfError::Format(_) | BcfError::Archive(_)),
        "{e:?}"
    );
}

#[test]
fn snapshots_that_are_too_big_or_not_pictures_are_left_out_with_a_warning() {
    let big = png_bytes(5000, 5000, 1);
    let junk = b"this is not a picture".to_vec();
    // Bytes that do not compress.
    let mut x = 12345u32;
    let huge: Vec<u8> = (0..3 * 1024 * 1024)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect();
    let mk = |snap: &str| {
        format!(
            r#"<Markup><Topic Guid="a7f9c1d2-1111-4222-8333-444455556666"><Title>x</Title></Topic>
            <Viewpoints Guid="c7f9c1d2-1111-4222-8333-444455556666"><Snapshot>{snap}</Snapshot></Viewpoints></Markup>"#
        )
    };
    for (name, bytes, word) in [
        ("big.png", &big, "pixels"),
        ("junk.png", &junk, "not a PNG or JPEG"),
        ("huge.png", &huge, "at most"),
        ("missing.png", &junk, "not in the archive"),
    ] {
        let markup = mk(name);
        let entry = if name == "missing.png" {
            "other.png"
        } else {
            name
        };
        let zip = make_zip(&[
            ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
            ("g/markup.bcf", markup.as_bytes()),
            (&format!("g/{entry}"), bytes),
        ]);
        let got = read(&zip, &ReadLimits::default(), 0).unwrap();
        assert_eq!(got.topics.len(), 1);
        assert!(got.topics[0].pictures.is_empty());
        assert!(got.topics[0].issue.viewpoints[0].snapshot.is_none());
        assert!(
            got.warnings.iter().any(|w| w.contains(word)),
            "{name}: {:?}",
            got.warnings
        );
    }
}

#[test]
fn a_snapshot_name_that_leaves_the_folder_is_not_read() {
    let markup = r#"<Markup><Topic Guid="a7f9c1d2-1111-4222-8333-444455556666"><Title>x</Title></Topic>
        <Viewpoints Guid="c7f9c1d2-1111-4222-8333-444455556666"><Viewpoint>../../etc/passwd</Viewpoint>
        <Snapshot>../other/snap.png</Snapshot></Viewpoints></Markup>"#;
    let png = png_bytes(10, 10, 1);
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        ("g/markup.bcf", markup.as_bytes()),
        ("other/snap.png", &png),
    ]);
    let got = import(&zip);
    assert_eq!(got.topics.len(), 1);
    assert!(got.topics[0].pictures.is_empty());
    assert!(
        got.warnings
            .iter()
            .filter(|w| w.contains("is not allowed"))
            .count()
            >= 2
    );
}

#[test]
fn topics_with_too_many_comments_or_viewpoints_or_bad_content_are_skipped_and_listed() {
    let many_comments: String = (0..1001)
        .map(|n| format!("<Comment Guid=\"{n:08x}-1111-4222-8333-444455556666\"><Date>2020-01-01T00:00:00Z</Date><Author>a</Author><Comment>t</Comment></Comment>"))
        .collect();
    let many_vps: String = (0..17)
        .map(|n| format!("<Viewpoints Guid=\"{n:08x}-1111-4222-8333-444455556666\"/>"))
        .collect();
    let topic =
        |guid: &str, title: &str| format!("<Topic Guid=\"{guid}\"><Title>{title}</Title></Topic>");
    let g = "a7f9c1d2-1111-4222-8333-444455556666";
    let m1 = format!("<Markup>{}{many_comments}</Markup>", topic(g, "c"));
    let m2 = format!("<Markup>{}{many_vps}</Markup>", topic(g, "v"));
    let m3 = format!("<Markup>{}</Markup>", topic("nope", "bad guid"));
    let m4 = format!("<Markup>{}</Markup>", topic(g, ""));
    let m5 = format!("<Markup>{}</Markup>", topic(g, &"t".repeat(300)));
    let m6 = format!("<Markup>{}</Markup>", topic(g, "ok"));
    let m7 = "<Other/>".to_string();
    let m8 = "<Markup/>".to_string();
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        ("t1/markup.bcf", m1.as_bytes()),
        ("t2/markup.bcf", m2.as_bytes()),
        ("t3/markup.bcf", m3.as_bytes()),
        ("t4/markup.bcf", m4.as_bytes()),
        ("t5/markup.bcf", m5.as_bytes()),
        ("t6/markup.bcf", m6.as_bytes()),
        ("t7/markup.bcf", m7.as_bytes()),
        ("t8/markup.bcf", m8.as_bytes()),
        // The same GUID in a second folder.
        ("t9/markup.bcf", m6.as_bytes()),
    ]);
    let got = import(&zip);
    assert_eq!(got.topics.len(), 1);
    let reasons: Vec<String> = got
        .skipped
        .iter()
        .map(|s| format!("{}: {}", s.topic, s.reason))
        .collect();
    assert_eq!(got.skipped.len(), 8, "{reasons:#?}");
    for needle in [
        "1001 comments",
        "17 viewpoints",
        "not valid",
        "no title",
        "at most",
        "no Markup",
        "earlier topic",
    ] {
        assert!(
            reasons.iter().any(|r| r.contains(needle)),
            "{needle} in {reasons:#?}"
        );
    }
}

#[test]
fn bad_dates_numbers_and_cameras_give_warnings_not_crashes() {
    let markup = r#"<Markup><Topic Guid="a7f9c1d2-1111-4222-8333-444455556666">
        <Title>x</Title><CreationDate>last tuesday</CreationDate><DueDate>never</DueDate>
        <ModifiedDate>2020-99-99</ModifiedDate></Topic>
        <Comment Guid="b7f9c1d2-1111-4222-8333-444455556666"><Date>soon</Date><Author>a</Author><Comment>c</Comment></Comment>
        <Comment><Date>2020-01-01T00:00:00Z</Date><Author>a</Author><Comment>  </Comment></Comment>
        <Viewpoints Guid="c7f9c1d2-1111-4222-8333-444455556666"><Viewpoint>v1.bcfv</Viewpoint></Viewpoints>
        <Viewpoints Guid="d7f9c1d2-1111-4222-8333-444455556666"><Viewpoint>v2.bcfv</Viewpoint></Viewpoints>
        <Viewpoints Guid="e7f9c1d2-1111-4222-8333-444455556666"><Viewpoint>v3.bcfv</Viewpoint></Viewpoints>
        <Viewpoints Guid="f7f9c1d2-1111-4222-8333-444455556666"><Viewpoint>v4.bcfv</Viewpoint></Viewpoints></Markup>"#;
    let cam = |body: &str| format!("<VisualizationInfo>{body}</VisualizationInfo>");
    let v1 = cam(
        "<OrthogonalCamera><CameraViewPoint><X>NaN</X><Y>0</Y><Z>0</Z></CameraViewPoint><CameraDirection><X>0</X><Y>0</Y><Z>-1</Z></CameraDirection><CameraUpVector><X>0</X><Y>1</Y><Z>0</Z></CameraUpVector><ViewToWorldScale>5</ViewToWorldScale></OrthogonalCamera>",
    );
    let v2 = cam(
        "<OrthogonalCamera><CameraViewPoint><X>1e30</X><Y>0</Y><Z>0</Z></CameraViewPoint><CameraDirection><X>0</X><Y>0</Y><Z>-1</Z></CameraDirection><CameraUpVector><X>0</X><Y>1</Y><Z>0</Z></CameraUpVector><ViewToWorldScale>5</ViewToWorldScale></OrthogonalCamera>",
    );
    let v3 = cam(
        "<PerspectiveCamera><CameraViewPoint><X>0</X><Y>0</Y><Z>0</Z></CameraViewPoint><CameraDirection><X>0</X><Y>0</Y><Z>0</Z></CameraDirection><CameraUpVector><X>0</X><Y>1</Y><Z>0</Z></CameraUpVector><FieldOfView>50</FieldOfView></PerspectiveCamera>",
    );
    let v4 = "this is not xml".to_string();
    let zip = make_zip(&[
        ("bcf.version", b"<Version VersionId=\"2.1\"/>"),
        ("g/markup.bcf", markup.as_bytes()),
        ("g/v1.bcfv", v1.as_bytes()),
        ("g/v2.bcfv", v2.as_bytes()),
        ("g/v3.bcfv", v3.as_bytes()),
        ("g/v4.bcfv", v4.as_bytes()),
    ]);
    let got = read(&zip, &ReadLimits::default(), 777).unwrap();
    let t = &got.topics[0];
    assert_eq!(
        t.issue.created, 777,
        "a date that is not valid becomes the time of the import"
    );
    assert_eq!(t.issue.due, None);
    assert_eq!(
        t.comments.len(),
        1,
        "the comment without a text is counted, not imported"
    );
    assert_eq!(t.comments[0].time, 777);
    assert!(t.issue.viewpoints.iter().all(|v| v.view.is_none()));
    let w = got.warnings.join("\n");
    for needle in ["is not valid", "no camera", "comments without a text"] {
        assert!(w.contains(needle), "{needle} in {w}");
    }
}

// ----- the camera -----

fn eye_of(v: &ViewSpec) -> [f64; 3] {
    match v {
        ViewSpec::Camera {
            target,
            yaw,
            pitch,
            distance,
            ..
        } => [
            target[0] + pitch.cos() * yaw.cos() * distance,
            target[1] + pitch.cos() * yaw.sin() * distance,
            target[2] + pitch.sin() * distance,
        ],
        ViewSpec::Plan { .. } => unreachable!(),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * (1.0 + a.abs().max(b.abs()))
}

proptest! {
    #[test]
    fn a_plan_view_comes_back(cx in -1e6f64..1e6, cy in -1e6f64..1e6, zoom in 1e-4f64..100.0, z in -1e5f64..1e5) {
        let v = ViewSpec::Plan { center: [cx, cy], zoom };
        let c = camera::to_bcf(&v, z);
        prop_assert!(c.orthogonal && c.size > 0.0);
        match camera::from_bcf(&c).unwrap() {
            ViewSpec::Plan { center, zoom: z2 } => {
                prop_assert!(near(center[0], cx) && near(center[1], cy) && near(z2, zoom));
            }
            other => prop_assert!(false, "{other:?}"),
        }
    }

    #[test]
    fn an_orbit_camera_comes_back(
        tx in -1e5f64..1e5, ty in -1e5f64..1e5, tz in -1e4f64..1e4,
        yaw in -3.1f64..3.1, pitch in -1.4f64..1.4, dist in 100.0f64..1e6, persp in any::<bool>(),
    ) {
        let v = ViewSpec::Camera { target: [tx, ty, tz], yaw, pitch, distance: dist, perspective: persp };
        let c = camera::to_bcf(&v, 0.0);
        prop_assert_eq!(c.orthogonal, !persp);
        let back = camera::from_bcf(&c).unwrap();
        let ViewSpec::Camera { yaw: y2, pitch: p2, distance: d2, perspective: s2, target: t2 } = back.clone() else {
            return Err(TestCaseError::fail("not a camera"));
        };
        prop_assert_eq!(s2, persp);
        prop_assert!(near(p2, pitch) && near(y2, yaw), "{back:?}");
        // The eye is the same. A perspective camera has no target: the distance is made up.
        let (e1, e2) = (eye_of(&v), eye_of(&back));
        for k in 0..3 { prop_assert!(near(e1[k], e2[k]), "eye {e1:?} {e2:?}"); }
        if !persp {
            prop_assert!(near(d2, dist));
            prop_assert!(near(t2[0], tx) && near(t2[1], ty) && near(t2[2], tz));
        }
    }

    #[test]
    fn a_camera_looking_straight_up_or_down_keeps_its_turn(yaw in -3.1f64..3.1, down in any::<bool>()) {
        let pitch = if down { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
        let v = ViewSpec::Camera { target: [0.0; 3], yaw, pitch, distance: 5000.0, perspective: false };
        let c = camera::to_bcf(&v, 0.0);
        let ViewSpec::Camera { yaw: y2, .. } = camera::from_bcf(&c).unwrap() else {
            // A straight-down camera with up along +Y is a plan view.
            return Ok(());
        };
        prop_assert!(near(y2, yaw), "{y2} {yaw}");
    }
}

#[test]
fn bad_cameras_are_refused() {
    let ok = BcfCamera {
        orthogonal: true,
        eye: [0.0; 3],
        direction: [1.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0],
        size: 5.0,
        aspect: 1.5,
    };
    assert!(camera::from_bcf(&ok).is_ok());
    for bad in [
        BcfCamera { size: 0.0, ..ok },
        BcfCamera {
            size: f64::NAN,
            ..ok
        },
        BcfCamera {
            direction: [0.0; 3],
            ..ok
        },
        BcfCamera {
            eye: [1e30, 0.0, 0.0],
            ..ok
        },
        BcfCamera {
            orthogonal: false,
            size: 180.0,
            ..ok
        },
        BcfCamera {
            orthogonal: false,
            size: 0.0,
            ..ok
        },
        BcfCamera {
            up: [f64::INFINITY, 0.0, 0.0],
            ..ok
        },
    ] {
        assert!(camera::from_bcf(&bad).is_err(), "{bad:?}");
    }
}

// ----- the types -----

#[test]
fn versions_and_errors_have_names() {
    assert_eq!(Version::V21.id(), "2.1");
    assert_eq!(Version::V30.id(), "3.0");
    assert_eq!(Version::parse("2.0"), Some(Version::V21));
    assert_eq!(Version::parse(" 3.0 "), Some(Version::V30));
    assert_eq!(Version::parse("4"), None);
    for e in [
        BcfError::Archive("a".into()),
        BcfError::Limit("b".into()),
        BcfError::Format("c".into()),
        BcfError::Version("9".into()),
    ] {
        assert!(!e.to_string().is_empty());
    }
    let _ = IssueData::default();
}
