//! Commands for review issues (REV-04): issues, their comments and their viewpoints.
//!
//! Every command is one undo step. A snapshot is a PNG picture. It becomes an `image` resource of
//! the document (once per SHA-256), and the viewpoint holds the hash. A command that removes the
//! last viewpoint that uses a snapshot does NOT remove the resource: in a shared document another
//! user may still point to it. An unused picture is removed by an explicit purge (`resource.purge`,
//! or `issue.delete {purge_snapshots}`), which is one undo step.

use super::*;
use crate::issue::{
    Comment, CommentId, Issue, IssueId, MAX_COMMENTS, MAX_COMMENTS_TOTAL, MAX_ISSUES,
    MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_PIXELS, MAX_VIEWPOINTS, Viewpoint, ViewpointId, is_unset,
};
use crate::picture::ImageResource;

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl Tx<'_> {
    fn issue_index(&self, id: IssueId) -> Result<usize, CommandError> {
        self.doc
            .issues
            .data()
            .issues
            .iter()
            .position(|i| i.id == id)
            .ok_or(CommandError::NoIssue(id))
    }

    /// Checks a snapshot file (a PNG or JPEG picture) and stores it as a picture resource (once per
    /// hash).
    fn snapshot_picture(&mut self, p: &ImageResource) -> Result<(), CommandError> {
        if p.byte_len() > MAX_SNAPSHOT_BYTES {
            return Err(invalid(format!(
                "a snapshot is at most {MAX_SNAPSHOT_BYTES} bytes"
            )));
        }
        if u64::from(p.width) * u64::from(p.height) > MAX_SNAPSHOT_PIXELS {
            return Err(invalid(format!(
                "a snapshot has at most {MAX_SNAPSHOT_PIXELS} pixels"
            )));
        }
        let mut scratch = Outcome::default();
        self.picture_resource(p, &mut scratch)?;
        Ok(())
    }

    /// Stores the pictures of the new viewpoints and checks that every snapshot hash has a picture.
    fn store_snapshots(
        &mut self,
        viewpoints: &[Viewpoint],
        pictures: &[&ImageResource],
    ) -> Result<(), CommandError> {
        for p in pictures {
            self.snapshot_picture(p)?;
        }
        for v in viewpoints {
            if let Some(h) = &v.snapshot
                && self.doc.resources.find_image(h).is_none()
            {
                return Err(invalid(format!(
                    "the snapshot {h} has no picture: pass it in `pictures`"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn run_issue(
        &mut self,
        cmd: &Command,
        out: &mut Outcome,
    ) -> Result<(), CommandError> {
        match cmd {
            Command::AddIssue { issue, pictures } => {
                let mut issue: Issue = (**issue).clone();
                if self.doc.issues.len() >= MAX_ISSUES {
                    return Err(invalid(format!(
                        "a document has at most {MAX_ISSUES} issues"
                    )));
                }
                if is_unset(issue.id.0) {
                    issue.id = IssueId(self.alloc()?);
                } else if self.doc.issues.get(issue.id).is_some() {
                    return Err(invalid(format!("issue {} exists already", issue.id)));
                }
                out.created.push(issue.id.0);
                for v in &mut issue.viewpoints {
                    if is_unset(v.id.0) {
                        v.id = ViewpointId(self.alloc()?);
                    }
                    out.created.push(v.id.0);
                }
                issue.validate().map_err(invalid)?;
                let refs: Vec<&ImageResource> = pictures.iter().collect();
                self.store_snapshots(&issue.viewpoints, &refs)?;
                self.doc.issues.insert_issue(issue);
            }
            Command::ModifyIssue { id, patch, now } => {
                let at = self.issue_index(*id)?;
                let old = &self.doc.issues.data().issues[at];
                let mut new = patch.apply(old);
                if new == *old {
                    return Ok(());
                }
                new.modified = *now;
                new.validate().map_err(invalid)?;
                self.doc.issues.data_mut().issues[at] = new;
            }
            Command::DeleteIssue { id } => {
                let at = self.issue_index(*id)?;
                let d = self.doc.issues.data_mut();
                d.issues.remove(at);
                d.comments.retain(|c| c.issue != *id);
            }
            Command::AddIssueComment { comment } => {
                let mut c: Comment = (**comment).clone();
                let at = self.issue_index(c.issue)?;
                let data = self.doc.issues.data();
                if data.comments.len() >= MAX_COMMENTS_TOTAL {
                    return Err(invalid(format!(
                        "a document has at most {MAX_COMMENTS_TOTAL} comments"
                    )));
                }
                if self.doc.issues.comment_count(c.issue) >= MAX_COMMENTS {
                    return Err(invalid(format!(
                        "an issue has at most {MAX_COMMENTS} comments"
                    )));
                }
                if let Some(v) = c.viewpoint
                    && !data.issues[at].viewpoints.iter().any(|x| x.id == v)
                {
                    return Err(invalid(format!("the issue has no viewpoint {v}")));
                }
                if is_unset(c.id.0) {
                    c.id = CommentId(self.alloc()?);
                } else if self.doc.issues.comment(c.id).is_some() {
                    return Err(invalid(format!("comment {} exists already", c.id)));
                }
                c.validate().map_err(invalid)?;
                out.created.push(c.id.0);
                let d = self.doc.issues.data_mut();
                d.issues[at].modified = d.issues[at].modified.max(c.time);
                self.doc.issues.insert_comment(c);
            }
            Command::EditIssueComment { id, text, now } => {
                let at = self
                    .doc
                    .issues
                    .data()
                    .comments
                    .iter()
                    .position(|c| c.id == *id)
                    .ok_or(CommandError::NoComment(*id))?;
                let mut c = self.doc.issues.data().comments[at].clone();
                if c.text == *text {
                    return Ok(());
                }
                c.text = text.clone();
                c.edited = Some(*now);
                c.validate().map_err(invalid)?;
                self.doc.issues.data_mut().comments[at] = c;
            }
            Command::DeleteIssueComment { id } => {
                let at = self
                    .doc
                    .issues
                    .data()
                    .comments
                    .iter()
                    .position(|c| c.id == *id)
                    .ok_or(CommandError::NoComment(*id))?;
                self.doc.issues.data_mut().comments.remove(at);
            }
            Command::AddIssueViewpoint {
                issue,
                viewpoint,
                picture,
                now,
            } => {
                let at = self.issue_index(*issue)?;
                let mut v: Viewpoint = (**viewpoint).clone();
                let mut new = self.doc.issues.data().issues[at].clone();
                if new.viewpoints.len() >= MAX_VIEWPOINTS {
                    return Err(invalid(format!(
                        "an issue has at most {MAX_VIEWPOINTS} viewpoints"
                    )));
                }
                if is_unset(v.id.0) {
                    v.id = ViewpointId(self.alloc()?);
                } else if new.viewpoints.iter().any(|x| x.id == v.id) {
                    return Err(invalid(format!("viewpoint {} exists already", v.id)));
                }
                out.created.push(v.id.0);
                new.viewpoints.push(v);
                new.viewpoints.sort_by_key(|x| (x.time, x.id));
                new.modified = *now;
                new.validate().map_err(invalid)?;
                let pics: Vec<&ImageResource> = picture.iter().map(|p| p.as_ref()).collect();
                self.store_snapshots(&new.viewpoints, &pics)?;
                self.doc.issues.data_mut().issues[at] = new;
            }
            Command::ModifyIssueViewpoint {
                issue,
                viewpoint,
                picture,
                now,
            } => {
                let at = self.issue_index(*issue)?;
                let old = self.doc.issues.data().issues[at].clone();
                let Some(vat) = old.viewpoints.iter().position(|v| v.id == viewpoint.id) else {
                    return Err(invalid(format!(
                        "the issue has no viewpoint {}",
                        viewpoint.id
                    )));
                };
                if old.viewpoints[vat] == **viewpoint {
                    return Ok(());
                }
                let mut new = old.clone();
                new.viewpoints[vat] = (**viewpoint).clone();
                new.viewpoints.sort_by_key(|x| (x.time, x.id));
                new.modified = *now;
                new.validate().map_err(invalid)?;
                let pics: Vec<&ImageResource> = picture.iter().map(|p| p.as_ref()).collect();
                self.store_snapshots(&new.viewpoints, &pics)?;
                self.doc.issues.data_mut().issues[at] = new;
            }
            Command::DeleteIssueViewpoint {
                issue,
                viewpoint,
                now,
            } => {
                let at = self.issue_index(*issue)?;
                let d = self.doc.issues.data_mut();
                let i = &mut d.issues[at];
                let Some(vat) = i.viewpoints.iter().position(|v| v.id == *viewpoint) else {
                    return Err(invalid(format!("the issue has no viewpoint {viewpoint}")));
                };
                i.viewpoints.remove(vat);
                i.modified = *now;
                d.comments
                    .update_where(|c| c.viewpoint == Some(*viewpoint), |c| c.viewpoint = None);
            }
            _ => unreachable!("not an issue command"),
        }
        Ok(())
    }
}
