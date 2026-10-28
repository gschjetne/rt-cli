# Writing review documents

A review document is how a coding agent asks a human for decisions. The human reads it in
the browser with each decision rendered as a card they answer inline, and talks with the
agent alongside. This guide is for the agent writing it. `rt agent-instructions` prints the
short version for a project's `CLAUDE.md` or `AGENTS.md`.

## What a good review document does

The human arrives cold, possibly hours after you started. Write for that reader:

- **Lead with what changed.** What was built, what works, what does not, how to see it
  (a preview URL in the front matter, screenshots, commands). Two or three short
  paragraphs, not a changelog.
- **Ask only what you cannot decide.** Each decision is something where the human's
  judgement, taste or knowledge is needed, and where the answer changes what you do next.
  Do not ask for approval of things you are confident about; say what you did.
- **Make every decision answerable from its card.** The card shows the heading, the
  options and the text under the heading. State the trade-off there; link the evidence.
- **Recommend.** Say which option you would pick and why. The human can disagree in one
  click; a recommendation saves them reconstructing your reasoning.
- **Don't ask for acceptance in a decision.** The human accepts the whole review with
  the Accept button once you mark it ready (`rt review ready`). A `go-nogo` decision is
  for a specific piece of work: did it come out the way the human wanted?
- **Prefer a few decisions to many.** Five sharp questions get answered; twenty get
  skimmed. Group small related points into one decision, or note them as assumptions the
  human can object to.

## Format

Markdown with optional YAML front matter and fenced `decision` blocks. Fenced `mermaid`
blocks render as diagrams on the review page, in the document and in decision text alike;
there is no size limit to keep them small for. Keep the file, and
anything it links to, outside the repository: the review session is the system of record
and stores the document's revisions, its attachments and the closing
record, on the instance. Git holds the plan, not the reviews.

````markdown
---
work_package: m3            # groups sessions; created if missing (default: "default")
title: "Milestone 3: offline sync"
preview: https://wp-03.example.dev   # shown as a link at the top
project: my-app             # optional; normally inferred from the repository
---

## Context
Prose, lists, tables, code, images. Relative links and images are uploaded with the
document: ![conflict](shots/conflict.png), [the notes](research/crdt-eval.md).

## Decision: Conflict resolution strategy
```decision
id: conflict-strategy
kind: choice
options:
  - last-writer-wins (simplest, loses edits)
  - per-field merge (what the prototype does)
  - CRDT (needs a new dependency)
recommend: per-field merge
```
Text under the heading, before or after the block, is the decision's support material
and shows on its card.
````

A decision owns the heading right before its block, which becomes its title (a leading
`Decision:` or `Decision 4:` is dropped), and the text up to the next heading of the same
or a higher level. Deeper headings inside that text stay part of the decision.

### Decision block keys

| Key | Meaning |
|---|---|
| `id` | Required. Stable identifier: lowercase letters, digits, `-`, `_`, `.`. Keep it when you revise the decision. |
| `kind` | Required. `go-nogo`, `choice` or `text`. `multi-choice` is `choice` with `multi: true`. |
| `options` | For `choice`: at least two, one line each. |
| `multi` | For `choice`: `true` lets the human pick several. |
| `recommend` | For `choice`: an option by its text, its letter (`B`), or a unique prefix of at least two characters; a list for `multi`. For `go-nogo`: `go` or `no-go`. For `text`: a suggested answer. |
| `title` | Overrides the heading, or gives a title to a block without one. |
| `withdrawn` | Withdraws the decision; the value is the reason shown to the human. |

Every card also has a free-text box, so the human can always answer "neither, because…".
Do not add an "other" option for that.

Numbers are not written in the document. The session numbers decisions in order of first
appearance and never reuses a number, so "decision 4" means the same thing for the whole
session even after others are withdrawn. Refer to decisions by number.

### Linking to other reviews

A link whose target is `review:<id>` goes to another review on the same instance;
`rt review list` shows the ids. Add `#<section>` to go to a part of it:

| Target | Goes to |
|---|---|
| `review:k3x9tq2m` | The review |
| `review:k3x9tq2m#4` | Its decision 4 |
| `review:k3x9tq2m#cache-strategy` | Its decision with the id `cache-strategy` |
| `review:k3x9tq2m#cache-keys` | Its heading `## Cache keys`: lower case, spaces as hyphens, punctuation dropped, as on GitHub. A repeated heading gets `-1`, `-2`… |

`[the caching review](review:k3x9tq2m)` shows your words; `<review:k3x9tq2m#4>` shows
the review's title and the decision, to readers who may see that review. The review's
page address (`https://<instance>/sessions/k3x9tq2m#d-4`) works too, as the human copies
it from the browser.

The review you link to lists yours under "Referred to by", for readers who may see both.
That needs you to be able to see it; `rt review open` and `rt review update` warn about
a link to a review you cannot see, which stays a plain link. Links in the log's notes
point at the review but are not listed there.

`rt review open` and `rt review update` check the document before sending it and list
every problem with its line number.

## Revising

Answers change questions. When they do, edit the document and run
`rt review update review.md` (`--dry-run` first if unsure). Without a copy at hand,
`rt review document --out review.md` writes the current revision; linked files already
uploaded do not need to be next to it. The differ matches decisions
by `id`:

- **New id**: added with the next number.
- **Changed kind, title, options, recommendation or text**: a new version of the decision.
  Answers given to the old version are marked stale; the human confirms them with one
  click ("still stands") or changes them. Whitespace-only edits are not revisions.
- **`withdrawn: <reason>`** in the block, or the block removed: withdrawn, number kept,
  reason shown. Prefer the explicit form, so the human learns why.
- **A withdrawn id that reappears**: reinstated under its old number.

Then tell the human what you changed and why, and post a one-line note to the log. The human sees the change on the page
immediately, but not your reasoning.

Revise rather than ask a new question when the question is the same with better options.
Withdraw and add when the question itself changed.

## During the session

- The human answers on the page and talks to you in the harness. The page has no chat;
  its log keeps events and your notes.
- `rt review wait` returns when the human sends answers. If they arrive while you are
  working on a request from the human in the harness, finish and answer that first.
- `rt review state` shows unsent drafts too. You may start on work a draft points to,
  but treat only sent answers as decided.
- When the human answers in conversation ("go with redb"), record it against the
  decision (`rt review record 4 --choice redb`) so the form reflects it; they confirm with
  one click. The agent cannot overwrite an unsent draft of theirs.
- Post a one-line note to the log (`rt review post`) for anything settled in
  conversation, so the record has it. Long explanations belong in the document, next to
  the decision they explain.

## Closing

When every decision has a sent answer to its current version and you have nothing more to
add, run `rt review ready`. The human accepts with the Accept button next to Send, which
wakes your `rt review wait`. A Send or a revision clears "ready". After acceptance the
form is final; `rt review close` prints a Markdown record: the outcome table, each decision with its
answer and revision count, withdrawn decisions, and the log. It stays with the session,
linked from the review page; record what the decisions mean for the project in the plan,
not by committing the review.

## Handing off

A review session outlives agent sessions. If yours ends, the human opens "Start a new
agent session" on the page (or you run `rt review handoff`) and gives a fresh agent session
the one line it shows: run `rt review attach <id>` and follow its instructions. Attaching
prints the live state of the review, its repository and how to work it, and detaches yours: your next `rt review wait`
exits with status 3, and you should stop.
