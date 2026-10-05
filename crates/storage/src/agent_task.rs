//! Where the background agent is registered (ADR-0006, issue #87): a Task
//! Scheduler task of the signed-in user on Windows, through `schtasks.exe`.
//! The task runs `bardo --agent` when the user signs in (and every 15
//! minutes after, in case it stopped; a second copy is never started), as
//! the user, with no administrator rights: it reads the user's Credential
//! Manager like the app does.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bardo_domain::{AgentTask, AgentTaskError};

/// The argument that makes `bardo` run as the background agent.
pub const AGENT_ARGUMENT: &str = "--agent";

/// This platform's agent task: Task Scheduler on Windows. Elsewhere
/// (development builds) the task is kept in memory and nothing runs.
pub fn platform_agent_task() -> Box<dyn AgentTask> {
    #[cfg(windows)]
    {
        Box::new(TaskScheduler::for_this_user())
    }
    #[cfg(not(windows))]
    {
        Box::new(MemoryAgentTask::default())
    }
}

/// An agent task in process memory: registering and starting it only
/// records the call. For tests and non-Windows development.
#[derive(Default)]
pub struct MemoryAgentTask {
    state: Mutex<MemoryTask>,
}

#[derive(Default, Clone)]
struct MemoryTask {
    program: Option<PathBuf>,
    starts: usize,
    refuse: bool,
}

impl MemoryAgentTask {
    fn state(&self) -> std::sync::MutexGuard<'_, MemoryTask> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The program the task runs, while registered.
    pub fn program(&self) -> Option<PathBuf> {
        self.state().program.clone()
    }

    /// How many times the task was started by hand.
    pub fn starts(&self) -> usize {
        self.state().starts
    }

    /// Makes every change fail, as a system that refuses it would.
    pub fn refuse(&self, refuse: bool) {
        self.state().refuse = refuse;
    }

    fn change(&self, apply: impl FnOnce(&mut MemoryTask)) -> Result<(), AgentTaskError> {
        let mut state = self.state();
        if state.refuse {
            return Err(AgentTaskError("refused".into()));
        }
        apply(&mut state);
        Ok(())
    }
}

impl AgentTask for MemoryAgentTask {
    fn is_registered(&self) -> Result<bool, AgentTaskError> {
        Ok(self.state().program.is_some())
    }

    fn register(&self, program: &Path) -> Result<(), AgentTaskError> {
        self.change(|state| state.program = Some(program.to_owned()))
    }

    fn start(&self) -> Result<(), AgentTaskError> {
        let registered = self.state().program.is_some();
        if !registered {
            return Err(AgentTaskError("not registered".into()));
        }
        self.change(|state| state.starts += 1)
    }

    fn remove(&self) -> Result<(), AgentTaskError> {
        self.change(|state| state.program = None)
    }
}

/// The Task Scheduler definition of the agent for `user` (`DOMAIN\name`),
/// running `program --agent`. Written as XML because only XML sets every
/// option `schtasks` would otherwise leave at defaults that stop the agent
/// (on battery, after 72 hours).
pub fn task_definition(user: &str, program: &Path) -> String {
    let user = xml_escape(user);
    let program = xml_escape(&program.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Sends the posts scheduled in Bardo while Bardo is closed. Turn it off in Bardo, Settings, Publishing.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Repetition>
        <Interval>PT15M</Interval>
        <StopAtDurationEnd>false</StopAtDurationEnd>
      </Repetition>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{program}</Command>
      <Arguments>{AGENT_ARGUMENT}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            c => escaped.push(c),
        }
    }
    escaped
}

/// The task's name: one per Windows user, in the root folder, which a user
/// without administrator rights may write to.
pub fn task_name(user_name: &str) -> String {
    format!("Bardo publishing agent ({user_name})")
}

#[cfg(windows)]
pub use windows::TaskScheduler;

#[cfg(windows)]
mod windows {
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Output};

    use bardo_domain::{AgentTask, AgentTaskError};

    use super::{task_definition, task_name};

    /// No console window flashes when `schtasks` runs.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// The agent's task in Windows Task Scheduler, for the signed-in user.
    pub struct TaskScheduler {
        /// `DOMAIN\name`, which the task runs as.
        user: String,
        name: String,
    }

    impl TaskScheduler {
        pub fn for_this_user() -> Self {
            let name = std::env::var("USERNAME").unwrap_or_default();
            let user = match std::env::var("USERDOMAIN") {
                Ok(domain) if !domain.is_empty() => format!("{domain}\\{name}"),
                _ => name.clone(),
            };
            Self {
                user,
                name: task_name(&name),
            }
        }

        fn schtasks(&self, args: &[&str]) -> Result<Output, AgentTaskError> {
            Command::new("schtasks.exe")
                .args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map_err(|error| AgentTaskError(format!("could not run schtasks: {error}")))
        }

        fn run(&self, args: &[&str]) -> Result<(), AgentTaskError> {
            let output = self.schtasks(args)?;
            if output.status.success() {
                Ok(())
            } else {
                Err(AgentTaskError(format!(
                    "schtasks {} failed ({}): {}",
                    args.first().copied().unwrap_or_default(),
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                )))
            }
        }
    }

    impl AgentTask for TaskScheduler {
        fn is_registered(&self) -> Result<bool, AgentTaskError> {
            Ok(self
                .schtasks(&["/Query", "/TN", &self.name])?
                .status
                .success())
        }

        fn register(&self, program: &Path) -> Result<(), AgentTaskError> {
            // schtasks reads the definition from a file, in UTF-16 with its
            // byte order mark.
            let definition = task_definition(&self.user, program);
            let mut bytes = vec![0xFF, 0xFE];
            bytes.extend(definition.encode_utf16().flat_map(u16::to_le_bytes));
            let file = std::env::temp_dir().join(format!(
                "bardo-agent-task-{}.xml",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::write(&file, bytes)
                .map_err(|error| AgentTaskError(format!("could not write the task: {error}")))?;
            let file_arg = file.to_string_lossy().into_owned();
            let registered = self.run(&["/Create", "/TN", &self.name, "/XML", &file_arg, "/F"]);
            let _ = std::fs::remove_file(&file);
            registered
        }

        fn start(&self) -> Result<(), AgentTaskError> {
            self.run(&["/Run", "/TN", &self.name])
        }

        fn remove(&self) -> Result<(), AgentTaskError> {
            // A running agent is not ended here: it sees the setting off
            // within a few seconds and stops by itself, after giving back
            // the jobs it runs, rather than being cut off mid-upload.
            self.run(&["/Delete", "/TN", &self.name, "/F"])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_runs_the_agent_as_the_user_at_sign_in_without_limits() {
        let definition = task_definition(
            r"PC\João & Co",
            Path::new(r"C:\Users\João\AppData\Local\Bardo\bardo.exe"),
        );
        assert!(definition.contains(r"<UserId>PC\João &amp; Co</UserId>"));
        assert!(
            definition.contains(r"<Command>C:\Users\João\AppData\Local\Bardo\bardo.exe</Command>")
        );
        assert!(definition.contains("<Arguments>--agent</Arguments>"));
        assert!(definition.contains("<LogonTrigger>"));
        assert!(definition.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(definition.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(
            definition.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>")
        );
        assert!(definition.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(definition.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(definition.contains("<WakeToRun>false</WakeToRun>"));
    }

    #[test]
    fn each_windows_user_has_a_task_of_their_own() {
        assert_eq!(task_name("ana"), "Bardo publishing agent (ana)");
        assert_ne!(task_name("ana"), task_name("bia"));
    }

    #[test]
    fn the_memory_task_records_what_was_asked() {
        let task = MemoryAgentTask::default();
        assert!(!task.is_registered().unwrap());
        assert!(task.start().is_err(), "nothing to start");
        task.register(Path::new("/opt/bardo")).unwrap();
        assert_eq!(task.program(), Some(PathBuf::from("/opt/bardo")));
        task.start().unwrap();
        assert_eq!(task.starts(), 1);
        task.refuse(true);
        assert!(task.remove().is_err());
        task.refuse(false);
        task.remove().unwrap();
        assert!(!task.is_registered().unwrap());
    }
}
