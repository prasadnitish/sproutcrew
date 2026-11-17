//! The social outbox. A post goes to a destination only after a person approved its exact content,
//! destination, and time. Editing it afterwards removes the approval, and the send path checks the hash
//! again. A post that already went (or whose send was interrupted and may have gone) is never sent a second
//! time without a person confirming. Only fake destinations exist in this build, so nothing leaves the
//! machine: a real destination cannot be registered.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MAX_TEXT: usize = 5000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub destination: String,
    pub remote_id: String,
    pub sent_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Draft,
    Approved {
        by: String,
        hash: String,
    },
    /// The send began and the outcome is not known. A person must check the destination.
    Sending {
        hash: String,
    },
    Sent(Receipt),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub destination: String,
    pub text: String,
    pub scheduled_at: u64,
    pub status: Status,
    /// Who approved it, kept so a retry after a transient failure keeps the same approval.
    #[serde(default)]
    pub approved_by: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    /// Worth trying again later.
    Transient(String),
    Rejected(String),
}

pub trait Destination: Send {
    fn id(&self) -> &str;
    /// Only a stand-in that posts nowhere may be registered in this build.
    fn is_fake(&self) -> bool;
    fn max_chars(&self) -> usize;
    /// `key` is stable for the same content, so a destination that supports it can drop a repeat.
    fn send(&mut self, key: &str, text: &str, at: u64) -> Result<Receipt, SendError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    UnknownPost(String),
    UnknownDestination(String),
    NotFake(String),
    EmptyText,
    TooLong {
        max: usize,
    },
    NotApproved,
    /// The content changed after approval.
    ChangedSinceApproval,
    /// The same content for the same destination and time already exists as another post.
    Duplicate(String),
    AlreadySent,
    NeedsCheck,
    NotYetDue {
        at: u64,
    },
    WrongState(&'static str),
    NoApprover,
    /// The destination refused or failed. The post is marked failed.
    SendFailed(String),
    Io(String),
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    posts: Vec<Post>,
}

/// A service a post can go to, with the most characters one post may have there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    pub max_chars: usize,
}

/// X: 280 characters for an account without a paid plan. LinkedIn: 3,000 for a post.
pub const X: Service = Service {
    id: "x",
    name: "X",
    max_chars: 280,
};
pub const LINKEDIN: Service = Service {
    id: "linkedin",
    name: "LinkedIn",
    max_chars: 3000,
};

/// A stand-in for a real service that keeps its character limit and posts nowhere: it records each post in a
/// file in the project, so the drafting, approval and sending path can be used before any account is connected.
pub struct Rehearsal {
    service: Service,
    id: String,
    log: PathBuf,
}

impl Rehearsal {
    pub fn new(service: Service, project_root: &Path) -> Rehearsal {
        Rehearsal {
            id: format!("rehearsal-{}", service.id),
            log: project_root.join(format!("rehearsal-{}.log", service.id)),
            service,
        }
    }
}

impl Destination for Rehearsal {
    fn id(&self) -> &str {
        &self.id
    }
    fn is_fake(&self) -> bool {
        true
    }
    fn max_chars(&self) -> usize {
        self.service.max_chars
    }
    fn send(&mut self, key: &str, text: &str, at: u64) -> Result<Receipt, SendError> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .map_err(|e| SendError::Transient(e.to_string()))?;
        writeln!(f, "{at}\t{key}\t{}", text.replace('\n', " "))
            .map_err(|e| SendError::Transient(e.to_string()))?;
        Ok(Receipt {
            destination: self.id.clone(),
            remote_id: format!("rehearsal-{}", &key[..12.min(key.len())]),
            sent_at: at,
        })
    }
}

pub struct Outbox {
    path: PathBuf,
    saved: Saved,
    destinations: Vec<Box<dyn Destination>>,
}

/// A destination that posts nowhere: it appends each post to a log file in the project and gives a receipt.
/// It drops a repeat of the same key, as a real destination with an idempotency key would.
pub struct FakeFile {
    log: PathBuf,
}

pub const FAKE_FILE_ID: &str = "fake-file";

impl FakeFile {
    pub fn new(project_root: &Path) -> FakeFile {
        FakeFile {
            log: project_root.join("fake-destination.log"),
        }
    }
}

impl Destination for FakeFile {
    fn id(&self) -> &str {
        FAKE_FILE_ID
    }
    fn is_fake(&self) -> bool {
        true
    }
    fn max_chars(&self) -> usize {
        280
    }
    fn send(&mut self, key: &str, text: &str, at: u64) -> Result<Receipt, SendError> {
        use std::io::Write;
        let remote_id = format!("fake-{}", &key[..12.min(key.len())]);
        let existing = std::fs::read_to_string(&self.log).unwrap_or_default();
        if !existing
            .lines()
            .any(|l| l.starts_with(&format!("{remote_id}\t")))
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.log)
                .map_err(|e| SendError::Transient(e.to_string()))?;
            writeln!(f, "{remote_id}\t{at}\t{}", text.replace('\n', " "))
                .map_err(|e| SendError::Transient(e.to_string()))?;
        }
        Ok(Receipt {
            destination: FAKE_FILE_ID.into(),
            remote_id,
            sent_at: at,
        })
    }
}

/// What an approval covers: the destination, the exact text, and the time.
pub fn content_hash(destination: &str, text: &str, scheduled_at: u64) -> String {
    let mut h = Sha256::new();
    for part in [destination.as_bytes(), text.as_bytes()] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part);
    }
    h.update(scheduled_at.to_be_bytes());
    format!("{:x}", h.finalize())
}

fn state_word(s: &Status) -> &'static str {
    match s {
        Status::Draft => "draft",
        Status::Approved { .. } => "approved",
        Status::Sending { .. } => "sending",
        Status::Sent(_) => "sent",
        Status::Failed(_) => "failed",
        Status::Cancelled => "cancelled",
    }
}

impl Outbox {
    pub fn open(root: &Path) -> Result<Outbox, String> {
        let path = root.join("outbox.json");
        let saved = match std::fs::read_to_string(&path) {
            Ok(t) => serde_json::from_str(&t).map_err(|e| format!("outbox.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Saved::default(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(Outbox {
            path,
            saved,
            destinations: Vec::new(),
        })
    }

    fn save(&self) -> Result<(), Refusal> {
        let io = |e: std::io::Error| Refusal::Io(e.to_string());
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&self.saved).map_err(|e| Refusal::Io(e.to_string()))?,
        )
        .map_err(io)?;
        std::fs::rename(tmp, &self.path).map_err(io)
    }

    pub fn register(&mut self, d: Box<dyn Destination>) -> Result<(), Refusal> {
        if !d.is_fake() {
            return Err(Refusal::NotFake(d.id().to_string()));
        }
        self.destinations.retain(|x| x.id() != d.id());
        self.destinations.push(d);
        Ok(())
    }

    pub fn posts(&self) -> &[Post] {
        &self.saved.posts
    }

    fn index(&self, id: &str) -> Result<usize, Refusal> {
        self.saved
            .posts
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| Refusal::UnknownPost(id.to_string()))
    }

    fn check_content(
        &self,
        destination: &str,
        text: &str,
        at: u64,
        except: Option<&str>,
    ) -> Result<(), Refusal> {
        let d = self
            .destinations
            .iter()
            .find(|d| d.id() == destination)
            .ok_or_else(|| Refusal::UnknownDestination(destination.to_string()))?;
        if text.trim().is_empty() {
            return Err(Refusal::EmptyText);
        }
        if text.chars().count() > d.max_chars().min(MAX_TEXT) {
            return Err(Refusal::TooLong {
                max: d.max_chars().min(MAX_TEXT),
            });
        }
        let h = content_hash(destination, text, at);
        if let Some(other) = self.saved.posts.iter().find(|p| {
            Some(p.id.as_str()) != except
                && p.status != Status::Cancelled
                && content_hash(&p.destination, &p.text, p.scheduled_at) == h
        }) {
            return Err(Refusal::Duplicate(other.id.clone()));
        }
        Ok(())
    }

    pub fn draft(
        &mut self,
        destination: &str,
        text: &str,
        scheduled_at: u64,
    ) -> Result<String, Refusal> {
        self.check_content(destination, text, scheduled_at, None)?;
        let id = format!("p{}", self.saved.posts.len() + 1);
        self.saved.posts.push(Post {
            id: id.clone(),
            destination: destination.to_string(),
            text: text.to_string(),
            scheduled_at,
            status: Status::Draft,
            approved_by: None,
        });
        self.save()?;
        Ok(id)
    }

    /// Changing a post that was approved sends it back to draft.
    pub fn edit(&mut self, id: &str, text: &str, scheduled_at: u64) -> Result<(), Refusal> {
        let i = self.index(id)?;
        if matches!(
            self.saved.posts[i].status,
            Status::Sent(_) | Status::Sending { .. } | Status::Cancelled
        ) {
            return Err(Refusal::WrongState(state_word(&self.saved.posts[i].status)));
        }
        let dest = self.saved.posts[i].destination.clone();
        self.check_content(&dest, text, scheduled_at, Some(id))?;
        let p = &mut self.saved.posts[i];
        p.text = text.to_string();
        p.scheduled_at = scheduled_at;
        p.status = Status::Draft;
        p.approved_by = None;
        self.save()
    }

    pub fn approve(&mut self, id: &str, by: &str) -> Result<(), Refusal> {
        let i = self.index(id)?;
        if by.trim().is_empty() {
            return Err(Refusal::NoApprover);
        }
        let p = &mut self.saved.posts[i];
        if p.status != Status::Draft {
            return Err(Refusal::WrongState(state_word(&p.status)));
        }
        p.status = Status::Approved {
            by: by.trim().to_string(),
            hash: content_hash(&p.destination, &p.text, p.scheduled_at),
        };
        p.approved_by = Some(by.trim().to_string());
        self.save()
    }

    pub fn cancel(&mut self, id: &str) -> Result<(), Refusal> {
        let i = self.index(id)?;
        let p = &mut self.saved.posts[i];
        if matches!(p.status, Status::Sent(_) | Status::Sending { .. }) {
            return Err(Refusal::WrongState(state_word(&p.status)));
        }
        p.status = Status::Cancelled;
        self.save()
    }

    /// Sends one approved post whose time has come.
    pub fn send(&mut self, id: &str, now: u64) -> Result<Receipt, Refusal> {
        let i = self.index(id)?;
        let hash = match &self.saved.posts[i].status {
            Status::Draft => return Err(Refusal::NotApproved),
            Status::Sent(_) => return Err(Refusal::AlreadySent),
            Status::Sending { .. } => return Err(Refusal::NeedsCheck),
            s @ (Status::Failed(_) | Status::Cancelled) => {
                return Err(Refusal::WrongState(state_word(s)))
            }
            Status::Approved { hash, .. } => hash.clone(),
        };
        let (dest, text, at) = {
            let p = &self.saved.posts[i];
            (p.destination.clone(), p.text.clone(), p.scheduled_at)
        };
        if content_hash(&dest, &text, at) != hash {
            return Err(Refusal::ChangedSinceApproval);
        }
        if now < at {
            return Err(Refusal::NotYetDue { at });
        }
        let di = self
            .destinations
            .iter()
            .position(|d| d.id() == dest)
            .ok_or_else(|| Refusal::UnknownDestination(dest.clone()))?;
        // The intent is on disk before the destination is called, so a crash leaves a post that is not sent again by itself.
        self.saved.posts[i].status = Status::Sending { hash: hash.clone() };
        self.save()?;
        match self.destinations[di].send(&hash, &text, now) {
            Ok(r) => {
                self.saved.posts[i].status = Status::Sent(r.clone());
                self.save()?;
                Ok(r)
            }
            Err(e) => {
                let why = match e {
                    SendError::Transient(m) => format!("transient: {m}"),
                    SendError::Rejected(m) => format!("rejected: {m}"),
                };
                self.saved.posts[i].status = Status::Failed(why.clone());
                self.save()?;
                Err(Refusal::SendFailed(why))
            }
        }
    }

    /// Sends every approved post that is due. Posts left in an unknown state are reported, not sent.
    pub fn send_due(&mut self, now: u64) -> Vec<(String, Result<Receipt, Refusal>)> {
        let mut ids: Vec<(u64, String)> = self
            .saved
            .posts
            .iter()
            .filter(|p| match p.status {
                Status::Approved { .. } => p.scheduled_at <= now,
                Status::Sending { .. } => true,
                _ => false,
            })
            .map(|p| (p.scheduled_at, p.id.clone()))
            .collect();
        ids.sort();
        ids.into_iter()
            .map(|(_, id)| {
                let r = self.send(&id, now);
                (id, r)
            })
            .collect()
    }

    /// A person looked at the destination after an interrupted send. `posted` says whether it is there.
    pub fn resolve_uncertain(&mut self, id: &str, posted: Option<Receipt>) -> Result<(), Refusal> {
        let i = self.index(id)?;
        let p = &mut self.saved.posts[i];
        let Status::Sending { hash } = p.status.clone() else {
            return Err(Refusal::WrongState(state_word(&p.status)));
        };
        p.status = match posted {
            Some(r) => Status::Sent(r),
            None => Status::Approved {
                by: p.approved_by.clone().unwrap_or_default(),
                hash,
            },
        };
        self.save()
    }

    /// A post that failed for a passing reason goes back to approved, with the same approval, to be sent again.
    pub fn retry(&mut self, id: &str) -> Result<(), Refusal> {
        let i = self.index(id)?;
        let p = &mut self.saved.posts[i];
        match &p.status {
            Status::Failed(m) if m.starts_with("transient") => {}
            other => return Err(Refusal::WrongState(state_word(other))),
        }
        p.status = Status::Approved {
            by: p.approved_by.clone().unwrap_or_default(),
            hash: content_hash(&p.destination, &p.text, p.scheduled_at),
        };
        self.save()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rehearsals_keep_each_services_limit_and_post_nowhere() {
        let root = std::env::temp_dir().join(format!("bff-rehearsal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut o = Outbox::open(&root).unwrap();
        o.register(Box::new(Rehearsal::new(X, &root))).unwrap();
        o.register(Box::new(Rehearsal::new(LINKEDIN, &root)))
            .unwrap();
        assert_eq!(
            o.draft("rehearsal-x", &"a".repeat(281), 5),
            Err(Refusal::TooLong { max: 280 })
        );
        let id = o.draft("rehearsal-linkedin", &"a".repeat(2000), 5).unwrap();
        o.approve(&id, "Fixture owner").unwrap();
        let r = o.send(&id, 6).unwrap();
        assert!(r.remote_id.starts_with("rehearsal-"));
        assert!(std::fs::read_to_string(root.join("rehearsal-linkedin.log"))
            .unwrap()
            .contains("aaaa"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    use super::*;
    use std::sync::{Arc, Mutex};

    /// Records every send and can be told to fail.
    struct FakeX {
        sent: Arc<Mutex<Vec<(String, String)>>>,
        fail: Arc<Mutex<Vec<SendError>>>,
        fake: bool,
    }
    impl Destination for FakeX {
        fn id(&self) -> &str {
            "fake-x"
        }
        fn is_fake(&self) -> bool {
            self.fake
        }
        fn max_chars(&self) -> usize {
            280
        }
        fn send(&mut self, key: &str, text: &str, at: u64) -> Result<Receipt, SendError> {
            if let Some(e) = self.fail.lock().unwrap().pop() {
                return Err(e);
            }
            let mut sent = self.sent.lock().unwrap();
            sent.push((key.to_string(), text.to_string()));
            Ok(Receipt {
                destination: "fake-x".into(),
                remote_id: format!("post-{}", sent.len()),
                sent_at: at,
            })
        }
    }

    type Log = Arc<Mutex<Vec<(String, String)>>>;
    type Fails = Arc<Mutex<Vec<SendError>>>;

    fn outbox(tag: &str) -> (Outbox, Log, Fails) {
        let d = std::env::temp_dir().join(format!("bff-outbox-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (sent, fail): (Log, Fails) = (Arc::default(), Arc::default());
        let mut o = Outbox::open(&d).unwrap();
        o.register(Box::new(FakeX {
            sent: sent.clone(),
            fail: fail.clone(),
            fake: true,
        }))
        .unwrap();
        (o, sent, fail)
    }

    #[test]
    fn nothing_is_sent_without_an_approval_of_the_exact_content() {
        let (mut o, sent, _) = outbox("approval");
        let id = o.draft("fake-x", "Hello world", 100).unwrap();
        assert_eq!(o.send(&id, 200), Err(Refusal::NotApproved));
        assert_eq!(o.approve(&id, "  "), Err(Refusal::NoApprover));
        o.approve(&id, "Nitish").unwrap();
        let r = o.send(&id, 200).unwrap();
        assert_eq!(r.remote_id, "post-1");
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert!(matches!(&o.posts()[0].status, Status::Sent(x) if x == &r));
    }

    #[test]
    fn editing_after_approval_removes_the_approval_and_the_send_path_checks_the_hash_again() {
        let (mut o, sent, _) = outbox("integrity");
        let id = o.draft("fake-x", "Version one", 100).unwrap();
        o.approve(&id, "Nitish").unwrap();
        o.edit(&id, "Version two, never approved", 100).unwrap();
        assert_eq!(o.posts()[0].status, Status::Draft);
        assert_eq!(o.send(&id, 200), Err(Refusal::NotApproved));
        // Changing only the time also voids it.
        o.approve(&id, "Nitish").unwrap();
        o.edit(&id, "Version two, never approved", 500).unwrap();
        assert_eq!(o.posts()[0].status, Status::Draft);
        // A tampered file is caught at send time: the approval hash no longer matches the content.
        o.approve(&id, "Nitish").unwrap();
        o.saved.posts[0].text = "tampered".into();
        assert_eq!(o.send(&id, 600), Err(Refusal::ChangedSinceApproval));
        assert!(sent.lock().unwrap().is_empty());
    }

    #[test]
    fn a_post_is_not_sent_before_its_time() {
        let (mut o, sent, _) = outbox("time");
        let id = o.draft("fake-x", "Later", 1000).unwrap();
        o.approve(&id, "Nitish").unwrap();
        assert_eq!(o.send(&id, 999), Err(Refusal::NotYetDue { at: 1000 }));
        assert!(o.send_due(999).is_empty());
        let results = o.send_due(1000);
        assert_eq!(results.len(), 1);
        assert!(results[0].1.is_ok());
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert!(o.send_due(2000).is_empty(), "already sent");
    }

    #[test]
    fn a_post_that_already_went_is_never_sent_again_and_the_same_content_cannot_be_posted_twice() {
        let (mut o, sent, _) = outbox("dup");
        let a = o.draft("fake-x", "Same words", 100).unwrap();
        o.approve(&a, "Nitish").unwrap();
        let first = o.send(&a, 100).unwrap();
        assert_eq!(o.send(&a, 101), Err(Refusal::AlreadySent));
        // A second post with identical content, destination, and time is refused at drafting.
        assert_eq!(
            o.draft("fake-x", "Same words", 100),
            Err(Refusal::Duplicate(a.clone()))
        );
        // Different time or text is a different post.
        assert!(o.draft("fake-x", "Same words", 200).is_ok());
        assert!(o.draft("fake-x", "Other words", 100).is_ok());
        assert_eq!(sent.lock().unwrap().len(), 1);
        assert_eq!(first.remote_id, "post-1");
    }

    #[test]
    fn the_idempotency_key_is_stable_for_the_same_content_so_a_destination_can_drop_a_repeat() {
        let (mut o, sent, _) = outbox("key");
        let a = o.draft("fake-x", "Words", 100).unwrap();
        o.approve(&a, "Nitish").unwrap();
        o.send(&a, 100).unwrap();
        assert_eq!(
            sent.lock().unwrap()[0].0,
            content_hash("fake-x", "Words", 100)
        );
        assert_ne!(
            content_hash("fake-x", "Words", 100),
            content_hash("fake-x", "Words", 101)
        );
        assert_ne!(
            content_hash("fake-x", "Words", 100),
            content_hash("other", "Words", 100)
        );
        assert_ne!(
            content_hash("ab", "c", 1),
            content_hash("a", "bc", 1),
            "field boundaries cannot be forged"
        );
    }

    #[test]
    fn an_interrupted_send_is_not_retried_by_itself_and_a_person_settles_it() {
        let (mut o, sent, _) = outbox("crash");
        let id = o.draft("fake-x", "Maybe posted", 100).unwrap();
        o.approve(&id, "Nitish").unwrap();
        // Simulate a crash after the send began: the status was saved as Sending.
        let hash = content_hash("fake-x", "Maybe posted", 100);
        o.saved.posts[0].status = Status::Sending { hash };
        assert_eq!(o.send(&id, 200), Err(Refusal::NeedsCheck));
        assert!(o.send_due(200).iter().all(|(_, r)| r.is_err()));
        assert!(
            sent.lock().unwrap().is_empty(),
            "nothing was sent without a person looking"
        );
        // The person finds it did go out.
        let receipt = Receipt {
            destination: "fake-x".into(),
            remote_id: "found-it".into(),
            sent_at: 150,
        };
        o.resolve_uncertain(&id, Some(receipt.clone())).unwrap();
        assert_eq!(o.posts()[0].status, Status::Sent(receipt));
        // Or they find it did not, and it goes back to approved.
        let id2 = o.draft("fake-x", "Not posted", 100).unwrap();
        o.approve(&id2, "Nitish").unwrap();
        o.saved.posts[1].status = Status::Sending {
            hash: content_hash("fake-x", "Not posted", 100),
        };
        o.resolve_uncertain(&id2, None).unwrap();
        assert!(matches!(o.posts()[1].status, Status::Approved { .. }));
        assert!(o.send(&id2, 300).is_ok());
        assert!(
            o.resolve_uncertain(&id2, None).is_err(),
            "it is not uncertain any more"
        );
    }

    #[test]
    fn the_sending_state_is_saved_before_the_destination_is_called() {
        // A destination that checks what is on disk at the moment of the call.
        struct Peek(PathBuf);
        impl Destination for Peek {
            fn id(&self) -> &str {
                "peek"
            }
            fn is_fake(&self) -> bool {
                true
            }
            fn max_chars(&self) -> usize {
                100
            }
            fn send(&mut self, _: &str, _: &str, at: u64) -> Result<Receipt, SendError> {
                let on_disk = std::fs::read_to_string(&self.0).unwrap();
                assert!(
                    on_disk.contains("Sending"),
                    "the intent must be on disk first: {on_disk}"
                );
                Ok(Receipt {
                    destination: "peek".into(),
                    remote_id: "1".into(),
                    sent_at: at,
                })
            }
        }
        let d = std::env::temp_dir().join(format!("bff-outbox-{}-peek", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut o = Outbox::open(&d).unwrap();
        o.register(Box::new(Peek(d.join("outbox.json")))).unwrap();
        let id = o.draft("peek", "x", 1).unwrap();
        o.approve(&id, "Nitish").unwrap();
        o.send(&id, 1).unwrap();
    }

    #[test]
    fn failures_are_recorded_and_a_retry_needs_no_new_approval_but_a_rejection_is_final_until_edited(
    ) {
        let (mut o, sent, fail) = outbox("fail");
        let id = o.draft("fake-x", "Try me", 100).unwrap();
        o.approve(&id, "Nitish").unwrap();
        fail.lock()
            .unwrap()
            .push(SendError::Transient("timeout".into()));
        assert!(o.send(&id, 100).is_err());
        assert!(matches!(&o.posts()[0].status, Status::Failed(m) if m.contains("timeout")));
        assert!(sent.lock().unwrap().is_empty());
        o.retry(&id).unwrap();
        assert!(
            matches!(o.posts()[0].status, Status::Approved { .. }),
            "same approval, same content"
        );
        assert!(o.send(&id, 101).is_ok());
        let id2 = o.draft("fake-x", "Bad content", 100).unwrap();
        o.approve(&id2, "Nitish").unwrap();
        fail.lock()
            .unwrap()
            .push(SendError::Rejected("policy".into()));
        assert!(o.send(&id2, 100).is_err());
        assert!(
            o.retry(&id2).is_err(),
            "a rejected post is not retried as it stands"
        );
        o.edit(&id2, "Better content", 100).unwrap();
        assert_eq!(o.posts()[1].status, Status::Draft);
    }

    #[test]
    fn only_registered_fake_destinations_work_and_text_is_bounded() {
        let (mut o, _, _) = outbox("rules");
        assert_eq!(
            o.draft("real-x", "hi", 1),
            Err(Refusal::UnknownDestination("real-x".into()))
        );
        let real = FakeX {
            sent: Arc::default(),
            fail: Arc::default(),
            fake: false,
        };
        assert_eq!(
            o.register(Box::new(real)),
            Err(Refusal::NotFake("fake-x".into()))
        );
        assert_eq!(o.draft("fake-x", "   ", 1), Err(Refusal::EmptyText));
        assert_eq!(
            o.draft("fake-x", &"x".repeat(281), 1),
            Err(Refusal::TooLong { max: 280 })
        );
        assert!(o.draft("fake-x", &"x".repeat(280), 1).is_ok());
        assert_eq!(
            o.approve("nope", "N"),
            Err(Refusal::UnknownPost("nope".into()))
        );
    }

    #[test]
    fn a_cancelled_post_never_sends_and_everything_survives_a_reopen() {
        let (mut o, sent, _) = outbox("persist");
        let a = o.draft("fake-x", "Keep", 1).unwrap();
        let b = o.draft("fake-x", "Cancel me", 1).unwrap();
        o.approve(&a, "Nitish").unwrap();
        o.approve(&b, "Nitish").unwrap();
        o.cancel(&b).unwrap();
        assert_eq!(o.send(&b, 5), Err(Refusal::WrongState("cancelled")));
        o.send(&a, 5).unwrap();
        let root = o.path.parent().unwrap().to_path_buf();
        let again = Outbox::open(&root).unwrap();
        assert_eq!(again.posts(), o.posts());
        assert_eq!(sent.lock().unwrap().len(), 1);
        // A sent post cannot be edited or cancelled.
        assert!(o.edit(&a, "changed", 1).is_err());
        assert!(o.cancel(&a).is_err());
    }
}
