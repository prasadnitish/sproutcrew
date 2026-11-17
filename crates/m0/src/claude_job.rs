//! Builds and runs one Claude job under the section 12 contract. The prompt goes in on stdin.

use crate::claude_run::{run, ClaudeRun, RunSpec};
use crate::common::{claude_path, worker_env};
use crate::work::{Work, CLAUDE_ENV};
use bff_core::claude::{
    launch_args, mcp_config_json, settings_json, Event, Launch, Policy, Session, SystemPrompt,
};
use serde_json::{json, Value};
use std::io;
use std::time::Duration;

pub enum SessionKind {
    New(String),
    Resume(String),
}

pub struct Job<'a> {
    pub w: &'a Work,
    pub tools: Vec<&'a str>,
    /// `(name, command, args)` of one stdio MCP server.
    pub mcp: Option<(&'a str, String, Vec<String>)>,
    pub system_prompt: &'a str,
    pub json_schema: Option<Value>,
    pub max_turns: u32,
    pub model: &'a str,
    pub effort: &'a str,
    pub session: SessionKind,
    /// Send the prompt as a stream-json message after `system/init` has been seen and checked.
    pub stream_input: bool,
    pub prompt: String,
    pub timeout: Duration,
}

pub fn user_message_line(prompt: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": prompt}}).to_string()
}

pub fn args_for(job: &Job) -> io::Result<Vec<String>> {
    let (settings, mcp, role) = (
        job.w.run.join("settings.json"),
        job.w.run.join("mcp.json"),
        job.w.run.join("role.md"),
    );
    std::fs::write(&settings, settings_json().to_string())?;
    let server = job.mcp.as_ref().map(|(n, c, a)| {
        (
            *n,
            c.as_str(),
            a.iter().map(String::as_str).collect::<Vec<_>>(),
        )
    });
    std::fs::write(
        &mcp,
        mcp_config_json(server.as_ref().map(|(n, c, a)| (*n, *c, a.as_slice()))).to_string(),
    )?;
    std::fs::write(&role, job.system_prompt)?;
    let (settings_s, mcp_s, role_s) = (
        settings.to_string_lossy(),
        mcp.to_string_lossy(),
        role.to_string_lossy(),
    );
    let schema = job.json_schema.as_ref().map(Value::to_string);
    let session = match &job.session {
        SessionKind::New(id) => Session::New(id),
        SessionKind::Resume(id) => Session::Resume(id),
    };
    Ok(launch_args(&Launch {
        tools: &job.tools,
        settings_path: &settings_s,
        mcp_config_path: &mcp_s,
        model: job.model,
        effort: job.effort,
        max_turns: job.max_turns,
        system_prompt: SystemPrompt::File(&role_s),
        json_schema: schema.as_deref(),
        session,
        stream_input: job.stream_input,
    }))
}

/// What a run may have. `--json-schema` makes the CLI add a `StructuredOutput` tool, and nothing else.
pub fn policy_for(job: &Job) -> Policy {
    Policy {
        tools: job.tools.iter().map(|s| s.to_string()).collect(),
        mcp_servers: job.mcp.iter().map(|(n, _, _)| n.to_string()).collect(),
        implicit: if job.json_schema.is_some() {
            vec!["StructuredOutput".to_string()]
        } else {
            vec![]
        },
    }
}

pub fn run_job(job: &Job, interrupt: Option<&dyn Fn(&Event) -> bool>) -> io::Result<ClaudeRun> {
    let args = args_for(job)?;
    let env = worker_env(&job.w.tmp, CLAUDE_ENV);
    let policy = policy_for(job);
    let program = claude_path();
    let text = if job.stream_input {
        user_message_line(&job.prompt)
    } else {
        job.prompt.clone()
    };
    let mut spec = RunSpec::new(
        &program,
        &args,
        &env,
        &job.w.t1,
        &policy,
        job.timeout,
        Duration::from_secs(5),
    )
    .stdin(&text);
    if job.stream_input {
        spec = spec.defer_until_init(Duration::from_secs(10));
    }
    if let Some(f) = interrupt {
        spec = spec.interrupt_when(f);
    }
    run(&spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Dirs;
    use crate::work;

    #[test]
    fn a_job_writes_its_run_files_outside_the_task_folder_and_never_puts_the_prompt_in_argv() {
        let d = Dirs {
            repo: std::env::temp_dir(),
            work: std::env::temp_dir().join(format!("bff-job-{}", std::process::id())),
        };
        let w = work::make(&d, "J").unwrap();
        let job = Job {
            w: &w,
            tools: vec!["Read", "Glob"],
            mcp: Some(("fake", "/bin/m0".into(), vec!["__fake-mcp".into()])),
            system_prompt: "be brief",
            json_schema: None,
            max_turns: 2,
            model: "claude-haiku-5-5",
            effort: "low",
            session: SessionKind::New(work::uuid_v4()),
            stream_input: true,
            prompt: "THE SECRET TASK PROMPT".into(),
            timeout: Duration::from_secs(10),
        };
        let args = args_for(&job).unwrap();
        assert!(args.iter().all(|a| !a.contains("SECRET")));
        assert!(w.run.join("settings.json").exists() && w.run.join("mcp.json").exists());
        assert_eq!(
            std::fs::read_to_string(w.run.join("role.md")).unwrap(),
            "be brief"
        );
        let mcp: Value =
            serde_json::from_str(&std::fs::read_to_string(w.run.join("mcp.json")).unwrap())
                .unwrap();
        assert_eq!(mcp["mcpServers"]["fake"]["command"], "/bin/m0");
        assert!(args
            .windows(2)
            .any(|p| p[0] == "--input-format" && p[1] == "stream-json"));
        std::fs::remove_dir_all(&d.work).unwrap();
    }

    #[test]
    fn structured_output_is_an_implicit_tool_only_when_a_schema_is_requested() {
        let d = Dirs {
            repo: std::env::temp_dir(),
            work: std::env::temp_dir().join(format!("bff-job2-{}", std::process::id())),
        };
        let w = work::make(&d, "P").unwrap();
        let mut job = Job {
            w: &w,
            tools: vec!["Read"],
            mcp: None,
            system_prompt: "x",
            json_schema: None,
            max_turns: 1,
            model: "m",
            effort: "low",
            session: SessionKind::New("s".into()),
            stream_input: false,
            prompt: String::new(),
            timeout: Duration::from_secs(1),
        };
        assert!(policy_for(&job).implicit.is_empty());
        job.json_schema = Some(json!({"type": "object"}));
        assert_eq!(
            policy_for(&job).implicit,
            vec!["StructuredOutput".to_string()]
        );
        assert_eq!(policy_for(&job).tools, vec!["Read".to_string()]);
        std::fs::remove_dir_all(&d.work).unwrap();
    }

    #[test]
    fn the_user_message_is_one_json_line() {
        let l = user_message_line("hello\nworld");
        assert_eq!(l.lines().count(), 1);
        let v: Value = serde_json::from_str(&l).unwrap();
        assert_eq!(v["message"]["content"], "hello\nworld");
    }
}
