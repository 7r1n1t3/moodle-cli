//! `moodle-cli status`: open work per course.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::api::{Client, Course};
use crate::db::{Db, TodoRow};
use crate::fmt::{BOLD, DIM, GREEN, RED, YELLOW, col, when};

/// Days back to look for overdue items.
const LOOKBACK: i64 = 30;

pub async fn run(
    client: &Client,
    db: &mut Db,
    courses: &[Course],
    days: i64,
    now: i64,
) -> Result<()> {
    let from = now - LOOKBACK * 86400;
    let to = now + days * 86400;
    let ids: Vec<i64> = courses.iter().map(|c| c.id).collect();

    let (mut events, mut assigns) = if ids.is_empty() {
        (HashMap::new(), HashMap::new())
    } else {
        let events = client.action_events(&ids, from, to).await?;
        let assigns = client.assignments(&ids).await?;
        (
            events.into_iter().map(|g| (g.courseid, g.events)).collect(),
            assigns
                .into_iter()
                .map(|c| (c.id, c.assignments))
                .collect::<HashMap<_, _>>(),
        )
    };

    let known = db.todo_keys()?;
    let mut rows = Vec::new();
    let mut total_open = 0;

    for c in courses {
        let mut lines: Vec<(i64, String)> = Vec::new();
        let mut seen = HashSet::new();
        let push = |rows: &mut Vec<TodoRow>,
                    lines: &mut Vec<(i64, String)>,
                    row: TodoRow,
                    line: String| {
            let line = if known.contains(&row.key) {
                line
            } else {
                format!("{line} {}", col("(new!)", RED))
            };
            lines.push((row.due, line));
            rows.push(row);
        };

        for a in assigns.remove(&c.id).unwrap_or_default() {
            let due = a.duedate.unwrap_or(0);
            if due == 0 || !(from..=to).contains(&due) {
                continue;
            }
            seen.extend([a.id, a.cmid]);
            let st = client.submission_status(a.id).await?;
            let status = st
                .lastattempt
                .and_then(|l| l.submission)
                .and_then(|s| s.status)
                .unwrap_or_else(|| "new".into());
            let graded = st
                .feedback
                .and_then(|f| f.gradefordisplay)
                .filter(|g| !g.is_empty());
            let tag = if status == "submitted" {
                let mut t = col("✓ submitted", GREEN);
                if let Some(g) = graded {
                    t.push_str(&format!(", grade: {g}"));
                }
                t
            } else if due < now {
                total_open += 1;
                col("✗ OVERDUE", RED)
            } else {
                total_open += 1;
                if status == "new" {
                    col("○ to do", YELLOW)
                } else {
                    col(&format!("○ {status}"), YELLOW)
                }
            };
            let line = format!("  {tag:<24} {}  — due {}", a.name, when(due, now));
            let row = TodoRow {
                key: format!("assign:{}", a.id),
                course_id: c.id,
                course_name: c.name().to_string(),
                kind: "assign",
                moodle_id: a.id,
                name: a.name,
                due,
                status,
            };
            push(&mut rows, &mut lines, row, line);
        }

        for e in events.remove(&c.id).unwrap_or_default() {
            if e.modulename.as_deref() == Some("assign")
                && e.instance.is_some_and(|i| seen.contains(&i))
            {
                continue;
            }
            if e.action.as_ref().and_then(|a| a.actionable) == Some(false) {
                continue;
            }
            let ts = e.timesort;
            let overdue = ts < now;
            let tag = if overdue {
                col("✗ OVERDUE", RED)
            } else {
                col("○ to do", YELLOW)
            };
            total_open += 1;
            let label = e
                .action
                .and_then(|a| a.name)
                .or(e.eventtype)
                .unwrap_or_default();
            let module = e.modulename.unwrap_or_default();
            let line = format!(
                "  {tag:<24} {} [{module}: {label}]  — {}",
                e.name,
                when(ts, now)
            );
            let row = TodoRow {
                key: format!("event:{}", e.id),
                course_id: c.id,
                course_name: c.name().to_string(),
                kind: "event",
                moodle_id: e.id,
                name: e.name,
                due: ts,
                status: if overdue { "overdue" } else { "todo" }.into(),
            };
            push(&mut rows, &mut lines, row, line);
        }

        println!(
            "{}{}",
            col(c.name(), BOLD),
            col(&format!("  ({})", c.viewurl), DIM)
        );
        if lines.is_empty() {
            println!("{}", col("  nothing due", DIM));
        } else {
            lines.sort();
            for (_, l) in lines {
                println!("{l}");
            }
        }
        println!();
    }

    db.upsert_todos(&rows, now)?;

    let summary = if total_open > 0 {
        format!("{total_open} open item(s) in the next {days} days")
    } else {
        "All clear 🎉".to_string()
    };
    println!("{}", col(&summary, BOLD));
    Ok(())
}
