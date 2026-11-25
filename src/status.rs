//! `moodle-cli status`: open work per course.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::api::{Client, Course, QuizAttempt};
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

    let (mut events, mut assigns, mut quizzes) = if ids.is_empty() {
        (HashMap::new(), HashMap::new(), HashMap::new())
    } else {
        let events = client.action_events(&ids, from, to).await?;
        let assigns = client.assignments(&ids).await?;
        let quizzes = client.quizzes(&ids).await?;
        (
            events.into_iter().map(|g| (g.courseid, g.events)).collect(),
            assigns
                .into_iter()
                .map(|c| (c.id, c.assignments))
                .collect::<HashMap<_, _>>(),
            quizzes
                .into_iter()
                .fold(HashMap::new(), |mut by_course, q| {
                    by_course.entry(q.course).or_insert_with(Vec::new).push(q);
                    by_course
                }),
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
                    let g = g.replace("&nbsp;", "");
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

        // Calendar action events contain only work Moodle currently considers
        // actionable. Fetch quizzes directly as well so completed quizzes and
        // quizzes omitted by the calendar provider are still shown.
        for q in quizzes.remove(&c.id).unwrap_or_default() {
            let due = q.timeclose.unwrap_or(0);
            if due == 0 || !(from..=to).contains(&due) {
                continue;
            }
            seen.extend([q.id, q.coursemodule]);
            let attempts = client.quiz_attempts(q.id).await?;
            let status = quiz_status(&attempts);
            let tag = if status == "submitted" {
                let mut tag = col("✓ submitted", GREEN);
                match client.quiz_grade(q.id).await {
                    Ok(Some(earned)) => {
                        let grade = format_quiz_grade(earned, q.max_grade, q.decimal_places);
                        tag.push_str(&format!(", grade: {grade}"));
                    }
                    Ok(None) => {}
                    Err(err) => eprintln!("Could not load grade for {}: {err}", q.name),
                }
                tag
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
            let line = format!("  {tag:<24} {} [quiz]  — due {}", q.name, when(due, now));
            let row = TodoRow {
                key: format!("quiz:{}", q.id),
                course_id: c.id,
                course_name: c.name().to_string(),
                kind: "quiz",
                moodle_id: q.id,
                name: q.name,
                due,
                status: status.into(),
            };
            push(&mut rows, &mut lines, row, line);
        }

        for e in events.remove(&c.id).unwrap_or_default() {
            if matches!(e.modulename.as_deref(), Some("assign" | "quiz"))
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

fn format_quiz_grade(earned: f64, maximum: Option<f64>, decimal_places: Option<usize>) -> String {
    let places = decimal_places.unwrap_or(2).min(5);
    let grade = match maximum {
        Some(max) => format!("{earned:.places$}/{max:.places$}"),
        None => format!("{earned:.places$}"),
    };
    grade.replace('.', ",")
}

fn quiz_status(attempts: &[QuizAttempt]) -> &str {
    if attempts.iter().any(|a| a.state == "finished") {
        "submitted"
    } else if attempts
        .iter()
        .any(|a| matches!(a.state.as_str(), "inprogress" | "overdue"))
    {
        "in progress"
    } else {
        "new"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_scaled_quiz_grade_including_zero() {
        assert_eq!(format_quiz_grade(1.0, Some(1.0), None), "1,00/1,00");
        assert_eq!(format_quiz_grade(0.0, Some(10.0), Some(1)), "0,0/10,0");
        assert_eq!(format_quiz_grade(2.5, None, Some(2)), "2,50");
    }

    fn attempt(state: &str) -> QuizAttempt {
        QuizAttempt {
            state: state.into(),
        }
    }

    #[test]
    fn quiz_status_uses_finished_attempt_even_if_another_is_open() {
        assert_eq!(
            quiz_status(&[attempt("finished"), attempt("inprogress")]),
            "submitted"
        );
    }

    #[test]
    fn quiz_status_recognises_open_and_missing_attempts() {
        assert_eq!(quiz_status(&[attempt("inprogress")]), "in progress");
        assert_eq!(quiz_status(&[]), "new");
    }
}
