//! Text renderings for the terminal. Agents read these, so they say what to do next.

use std::fmt::Write;

use jiff::Timestamp;
use rt_api::{
    Author, Binding, BindingStatus, Change, Decision, DecisionKind, DecisionStatus, Event,
    ResponseState, SessionState, SessionSummary, ThreadBody, ThreadItem,
};

pub fn json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub fn ago(t: Timestamp) -> String {
    let secs = (Timestamp::now().as_second() - t.as_second()).max(0);
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{}m ago", secs / 60),
        3600..86400 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}

fn author(item: &ThreadItem) -> String {
    let role = match item.author {
        Author::Human => "human",
        Author::Agent => "agent",
        Author::System => "system",
    };
    match &item.by {
        Some(by) => format!("{role} ({by})"),
        None => role.to_owned(),
    }
}

/// Who gave an answer, when the server says.
fn by(who: &Option<String>) -> String {
    who.as_ref()
        .map(|w| format!(" (by {w})"))
        .unwrap_or_default()
}

/// The sent answers among `items` that someone other than the session's owner gave: on a
/// cloud host, invited people's. Empty locally, where answers carry no names.
pub fn answers_not_by_owner(items: &[ThreadItem], owner: Option<&str>) -> Vec<String> {
    let Some(owner) = owner else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|i| match &i.body {
            ThreadBody::Event {
                event: Event::Sent { answers },
            } => Some(answers),
            _ => None,
        })
        .flatten()
        .filter_map(|a| {
            a.by.as_deref()
                .filter(|b| !b.eq_ignore_ascii_case(owner))
                .map(|b| format!("#{} by {b}", a.number))
        })
        .collect()
}

pub fn binding_line(b: &Binding) -> String {
    let status = match b.status {
        BindingStatus::Active => "active",
        BindingStatus::Unreachable => "unreachable",
        BindingStatus::Detached => "detached",
    };
    let mut line = format!(
        "{} {}",
        b.harness,
        b.session_ref.as_deref().unwrap_or("(no session id)")
    );
    let _ = write!(line, " — {status}, binding {}", b.id);
    if let Some(seen) = b.last_seen_at {
        let _ = write!(line, ", last seen {}", ago(seen));
    }
    line
}

pub fn counts_line(s: &SessionSummary) -> String {
    let c = &s.counts;
    let mut parts = vec![format!("{} answered", c.answered)];
    if c.drafts > 0 {
        parts.push(format!(
            "{} draft{}",
            c.drafts,
            if c.drafts == 1 { "" } else { "s" }
        ));
    }
    if c.stale > 0 {
        parts.push(format!("{} stale", c.stale));
    }
    if c.recorded_from_chat > 0 {
        parts.push(format!("{} recorded from chat", c.recorded_from_chat));
    }
    let unanswered = c.active - c.answered - c.drafts - c.stale - c.recorded_from_chat;
    if unanswered > 0 {
        parts.push(format!("{unanswered} unanswered"));
    }
    let mut line = format!("{} active: {}", c.active, parts.join(", "));
    if c.withdrawn > 0 {
        let _ = write!(line, "; {} withdrawn", c.withdrawn);
    }
    line
}

fn kind(d: &Decision) -> String {
    match d.content.kind {
        DecisionKind::Choice if d.content.multi => "multi-choice".to_owned(),
        k => k.to_string(),
    }
}

pub fn decision_block(out: &mut String, d: &Decision) {
    if d.status == DecisionStatus::Withdrawn {
        let _ = writeln!(
            out,
            "  {}. {} — withdrawn: {}",
            d.number,
            d.content.title,
            d.withdrawn_reason.as_deref().unwrap_or("no reason given")
        );
        return;
    }
    let _ = writeln!(
        out,
        "  {}. {}  [{} `{}`, v{}]",
        d.number,
        d.content.title,
        kind(d),
        d.id,
        d.version
    );
    let line = match &d.response {
        None => "unanswered".to_owned(),
        Some(r) => {
            let state = match r.state {
                ResponseState::Stale => format!(
                    "stale (answered v{}; awaiting the human's confirmation)",
                    r.version
                ),
                ResponseState::RecordedFromChat => {
                    "recorded from chat (awaiting the human's confirmation)".to_owned()
                }
                s => s.to_string(),
            };
            let mut line = format!("{state}: {}{}", r.answer.summary(), by(&r.by));
            if let Some(sent) = &r.last_sent {
                let _ = write!(line, "  (last sent: {})", sent.answer.summary());
            }
            line
        }
    };
    let _ = writeln!(out, "     {line}");
}

pub fn thread_item(out: &mut String, item: &ThreadItem, new: bool) {
    let mark = if new { "*" } else { " " };
    match &item.body {
        ThreadBody::Message { text, about } => {
            let about = if about.is_empty() {
                String::new()
            } else {
                format!(
                    " about {}",
                    about
                        .iter()
                        .map(|n| format!("#{n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            let _ = writeln!(
                out,
                "{mark} [{}] {}{about}, {}:",
                item.seq,
                author(item),
                ago(item.at)
            );
            for line in text.lines() {
                let _ = writeln!(out, "      {line}");
            }
        }
        ThreadBody::Event { event } => {
            let _ = writeln!(
                out,
                "{mark} [{}] {}: {}",
                item.seq,
                author(item),
                event_line(event)
            );
        }
    }
}

pub fn change_line(c: &Change) -> String {
    match c {
        Change::Added { number, title, id } => format!("added {number}. {title} (`{id}`)"),
        Change::Revised {
            number,
            title,
            version,
            fields,
            reinstated,
            ..
        } => {
            let mut s = format!("revised {number}. {title} to v{version}");
            if !fields.is_empty() {
                let _ = write!(s, " ({})", fields.join(", "));
            }
            if *reinstated {
                s.push_str(", reinstated");
            }
            s
        }
        Change::Withdrawn {
            number,
            title,
            reason,
            ..
        } => format!("withdrew {number}. {title} ({reason})"),
    }
}

pub fn event_line(event: &Event) -> String {
    match event {
        Event::Opened { decisions } => format!("opened the session with {decisions} decisions"),
        Event::DocumentRevised { revision, changes } if changes.is_empty() => {
            format!("revised the document (revision {revision}, text only)")
        }
        Event::DocumentRevised { revision, changes } => format!(
            "revised the document (revision {revision}): {}",
            changes
                .iter()
                .map(change_line)
                .collect::<Vec<_>>()
                .join("; ")
        ),
        Event::Sent { answers } => format!(
            "sent answers — {}",
            answers
                .iter()
                .map(|a| format!("#{} {}{}", a.number, a.answer.summary(), by(&a.by)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        Event::RecordedFromChat { number, answer, .. } => {
            format!(
                "recorded an answer to #{number} from chat: {}",
                answer.summary()
            )
        }
        Event::AgentAttached { binding, replaced } => format!(
            "attached agent session {} {}{}",
            binding.harness,
            binding.session_ref.as_deref().unwrap_or(""),
            if replaced.is_some() {
                " (replacing the previous one)"
            } else {
                ""
            }
        ),
        Event::AgentDetached { binding_id } => format!("detached agent binding {binding_id}"),
        Event::Ready { note } => match note {
            Some(n) => format!("marked the review ready to accept: {n}"),
            None => "marked the review ready to accept".to_owned(),
        },
        Event::Accepted => "accepted the review".to_owned(),
        Event::Invited { user } => format!("invited {} to answer", user.login),
        Event::Uninvited { user } => format!("withdrew the invitation of {}", user.login),
        Event::Closed => "closed the session".to_owned(),
    }
}

pub fn session_header(out: &mut String, s: &SessionSummary, url: &str) {
    let status = match s.status {
        rt_api::SessionStatus::Open => "open",
        rt_api::SessionStatus::Closed => "closed",
    };
    let _ = writeln!(out, "Review session {} — {}  [{status}]", s.id, s.title);
    let _ = writeln!(
        out,
        "Project {} · work package {} · document revision {}",
        s.project, s.work_package, s.document_revision
    );
    let _ = writeln!(out, "Page: {url}");
    if let Some(preview) = &s.preview {
        let _ = writeln!(out, "Preview: {preview}");
    }
    match &s.binding {
        Some(b) => {
            let _ = writeln!(out, "Agent: {}", binding_line(b));
        }
        None => out.push_str("Agent: none attached\n"),
    }
    if let Some(at) = s.accepted_at {
        let _ = writeln!(
            out,
            "Accepted by the human {}; close it when you have wrapped up.",
            ago(at)
        );
    } else if s.ready {
        let _ = writeln!(
            out,
            "Marked ready by the agent; {}",
            if s.all_answered {
                "the human can accept."
            } else {
                "the human can accept once every decision is answered."
            }
        );
    }
}

/// The full state: header, decisions, and the log (the last `tail` items plus anything
/// new to the caller).
pub fn state(state: &SessionState, url: &str, tail: Option<usize>) -> String {
    let mut out = String::new();
    session_header(&mut out, &state.session, url);
    let _ = writeln!(out, "\nDecisions ({})", counts_line(&state.session));
    for d in &state.decisions {
        decision_block(&mut out, d);
    }
    let seen = state.seen_up_to;
    let skip = tail.map_or(0, |t| state.thread.len().saturating_sub(t));
    let shown: Vec<_> = state
        .thread
        .iter()
        .enumerate()
        .filter(|(i, item)| *i >= skip || seen.is_some_and(|s| item.seq > s))
        .map(|(_, item)| item)
        .collect();
    let new_count = seen.map_or(0, |s| state.thread.iter().filter(|i| i.seq > s).count());
    let _ = write!(out, "\nLog");
    if shown.len() < state.thread.len() {
        let _ = write!(
            out,
            " (last {} of {}; --thread all for everything)",
            shown.len(),
            state.thread.len()
        );
    }
    if new_count > 0 {
        let _ = write!(out, " — {new_count} new, marked *");
    }
    out.push('\n');
    for item in shown {
        thread_item(&mut out, item, seen.is_some_and(|s| item.seq > s));
    }
    out
}
