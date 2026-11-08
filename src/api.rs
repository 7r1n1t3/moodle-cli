//! Minimal client for the Moodle web service REST API.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

const BASE: &str = "https://moodle.tu-darmstadt.de";

/// Form parameters in Moodle's flattened format (`name[i]` for lists).
#[derive(Default)]
pub struct Params(Vec<(String, String)>);

impl Params {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn arg(mut self, key: &str, value: impl ToString) -> Self {
        self.0.push((key.to_string(), value.to_string()));
        self
    }

    pub fn list<T: ToString>(mut self, key: &str, values: &[T]) -> Self {
        for (i, v) in values.iter().enumerate() {
            self.0.push((format!("{key}[{i}]"), v.to_string()));
        }
        self
    }
}

pub struct Client {
    http: reqwest::Client,
    token: String,
}

impl Client {
    pub fn new(token: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent(concat!("moodle-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http, token })
    }

    pub async fn call<T: DeserializeOwned>(&self, function: &str, params: Params) -> Result<T> {
        let mut form = vec![
            ("wstoken".to_string(), self.token.clone()),
            ("wsfunction".to_string(), function.to_string()),
            ("moodlewsrestformat".to_string(), "json".to_string()),
        ];
        form.extend(params.0);
        let res: serde_json::Value = self
            .http
            .post(format!("{BASE}/webservice/rest/server.php"))
            .form(&form)
            .send()
            .await
            .with_context(|| format!("{function}: request failed"))?
            .error_for_status()?
            .json()
            .await
            .with_context(|| format!("{function}: invalid JSON"))?;
        if res.get("exception").is_some_and(|e| !e.is_null()) {
            let msg = res.get("message").and_then(|m| m.as_str()).unwrap_or("?");
            bail!("{function}: {msg}");
        }
        serde_json::from_value(res).with_context(|| format!("{function}: unexpected response"))
    }

    pub async fn courses(&self, classification: &str) -> Result<Vec<Course>> {
        #[derive(Deserialize)]
        struct Resp {
            courses: Vec<Course>,
        }
        let r: Resp = self
            .call(
                "core_course_get_enrolled_courses_by_timeline_classification",
                Params::new().arg("classification", classification),
            )
            .await?;
        Ok(r.courses)
    }

    pub async fn action_events(
        &self,
        ids: &[i64],
        from: i64,
        to: i64,
    ) -> Result<Vec<CourseEvents>> {
        #[derive(Deserialize)]
        struct Resp {
            #[serde(default)]
            groupedbycourse: Vec<CourseEvents>,
        }
        let r: Resp = self
            .call(
                "core_calendar_get_action_events_by_courses",
                Params::new()
                    .list("courseids", ids)
                    .arg("timesortfrom", from)
                    .arg("timesortto", to)
                    .arg("limitnum", 50),
            )
            .await?;
        Ok(r.groupedbycourse)
    }

    pub async fn assignments(&self, ids: &[i64]) -> Result<Vec<AssignCourse>> {
        #[derive(Deserialize)]
        struct Resp {
            courses: Vec<AssignCourse>,
        }
        let r: Resp = self
            .call(
                "mod_assign_get_assignments",
                Params::new().list("courseids", ids),
            )
            .await?;
        Ok(r.courses)
    }

    pub async fn submission_status(&self, assign_id: i64) -> Result<SubmissionStatus> {
        self.call(
            "mod_assign_get_submission_status",
            Params::new().arg("assignid", assign_id),
        )
        .await
    }

    pub async fn course_contents(&self, course_id: i64) -> Result<Vec<Section>> {
        self.call(
            "core_course_get_contents",
            Params::new().arg("courseid", course_id),
        )
        .await
    }

    /// Download a file URL returned by the API.
    pub async fn download(&self, fileurl: &str) -> Result<Vec<u8>> {
        let url = authed_file_url(fileurl, &self.token)?;
        let bytes = self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(bytes.to_vec())
    }
}

/// Point a file URL at the web service endpoint and attach the token.
fn authed_file_url(fileurl: &str, token: &str) -> Result<url::Url> {
    let fixed = if fileurl.contains("/webservice/pluginfile.php") {
        fileurl.to_string()
    } else {
        fileurl.replacen("/pluginfile.php", "/webservice/pluginfile.php", 1)
    };
    let mut url = url::Url::parse(&fixed).with_context(|| format!("bad file URL: {fileurl}"))?;
    url.query_pairs_mut().append_pair("token", token);
    Ok(url)
}

/// First part of a course's full name (before `" - "`).
pub fn short_name(fullname: &str) -> &str {
    fullname.split(" - ").next().unwrap_or(fullname)
}

#[derive(Debug, Clone, Deserialize)]
pub struct Course {
    pub id: i64,
    pub fullname: String,
    #[serde(default)]
    pub viewurl: String,
}

impl Course {
    pub fn name(&self) -> &str {
        short_name(&self.fullname)
    }
}

#[derive(Debug, Deserialize)]
pub struct CourseEvents {
    pub courseid: i64,
    #[serde(default)]
    pub events: Vec<Event>,
}

#[derive(Debug, Deserialize)]
pub struct Event {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub modulename: Option<String>,
    #[serde(default)]
    pub instance: Option<i64>,
    pub timesort: i64,
    #[serde(default)]
    pub eventtype: Option<String>,
    #[serde(default)]
    pub action: Option<Action>,
}

#[derive(Debug, Deserialize)]
pub struct Action {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub actionable: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AssignCourse {
    pub id: i64,
    #[serde(default)]
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Deserialize)]
pub struct Assignment {
    pub id: i64,
    pub cmid: i64,
    pub name: String,
    #[serde(default)]
    pub duedate: Option<i64>,
    #[serde(default)]
    pub introattachments: Vec<FileEntry>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SubmissionStatus {
    #[serde(default)]
    pub lastattempt: Option<LastAttempt>,
    #[serde(default)]
    pub feedback: Option<Feedback>,
}

#[derive(Debug, Deserialize)]
pub struct LastAttempt {
    #[serde(default)]
    pub submission: Option<Submission>,
}

#[derive(Debug, Deserialize)]
pub struct Submission {
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Feedback {
    #[serde(default)]
    pub gradefordisplay: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Section {
    pub name: String,
    #[serde(default)]
    pub modules: Vec<Module>,
}

#[derive(Debug, Deserialize)]
pub struct Module {
    pub name: String,
    #[serde(default)]
    pub contents: Vec<FileEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileEntry {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    pub filename: String,
    #[serde(default)]
    pub filepath: Option<String>,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default)]
    pub fileurl: String,
    #[serde(default)]
    pub timemodified: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_flatten_lists() {
        let p = Params::new().arg("a", 1).list("ids", &[5, 6]);
        assert_eq!(
            p.0,
            vec![
                ("a".into(), "1".into()),
                ("ids[0]".into(), "5".into()),
                ("ids[1]".into(), "6".into()),
            ]
        );
    }

    #[test]
    fn file_url_gets_token() {
        let u = authed_file_url(
            "https://m.example/pluginfile.php/1/a.pdf?forcedownload=1",
            "T",
        )
        .unwrap();
        assert_eq!(
            u.as_str(),
            "https://m.example/webservice/pluginfile.php/1/a.pdf?forcedownload=1&token=T"
        );
        let u =
            authed_file_url("https://m.example/webservice/pluginfile.php/1/a.pdf", "T").unwrap();
        assert_eq!(
            u.as_str(),
            "https://m.example/webservice/pluginfile.php/1/a.pdf?token=T"
        );
    }

    #[test]
    fn course_short_name() {
        assert_eq!(
            short_name("Betriebssysteme - 20-00-0903-iv"),
            "Betriebssysteme"
        );
        assert_eq!(
            short_name("High-Performance Computing"),
            "High-Performance Computing"
        );
    }
}
