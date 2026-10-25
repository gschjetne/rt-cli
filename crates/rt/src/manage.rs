//! `rt wp` and `rt project`.

use anyhow::bail;
use clap::Subcommand;
use rt_api::{CreateWorkPackage, UpdateWorkPackage, WorkPackageStatus};
use rt_client::Client;

use crate::connect::connect;
use crate::output;
use crate::review::repo_root;

#[derive(Subcommand)]
pub enum WpCommand {
    /// Create a work package.
    Create {
        name: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        title: Option<String>,
        /// planning, building, reviewing, accepted, integrated or deployed.
        #[arg(long)]
        status: Option<WorkPackageStatus>,
    },
    /// List a project's work packages.
    List {
        #[arg(long)]
        project: Option<String>,
    },
    /// Show a work package and its review sessions, or set its status.
    Status {
        name: String,
        #[arg(long)]
        project: Option<String>,
        /// Set the status.
        #[arg(long)]
        set: Option<WorkPackageStatus>,
    },
}

#[derive(Subcommand)]
pub enum ProjectCommand {
    /// List the projects you can see.
    List,
}

/// The given project, or the project of the repository the current one's `origin` names.
fn project_for_cwd(client: &Client, given: Option<String>) -> anyhow::Result<String> {
    if let Some(p) = given {
        return Ok(p);
    }
    let repo = repo_root();
    let Some(full_name) =
        crate::review::origin_remote(&repo).and_then(|r| rt_api::remote::repo_full_name(&r))
    else {
        bail!("{repo} has no `origin` remote to find its project by; pass --project");
    };
    let found = client.projects()?.into_iter().find(|p| {
        p.repo
            .as_ref()
            .is_some_and(|r| r.full_name.eq_ignore_ascii_case(&full_name))
    });
    match found {
        Some(p) => Ok(p.name),
        None => bail!(
            "{full_name} is no project on {}: pass --project, or run `rt init` here",
            client.base()
        ),
    }
}

pub fn wp(cmd: WpCommand, json: bool) -> anyhow::Result<()> {
    let client = connect()?;
    match cmd {
        WpCommand::Create {
            name,
            project,
            title,
            status,
        } => {
            let project = project_for_cwd(&client, project)?;
            let wp = client.create_work_package(
                &project,
                &CreateWorkPackage {
                    name,
                    title,
                    status,
                },
            )?;
            if json {
                return output::json(&wp);
            }
            println!(
                "Created work package {}/{} ({}).",
                wp.project, wp.name, wp.status
            );
        }
        WpCommand::List { project } => {
            let project = project_for_cwd(&client, project)?;
            let wps = client.work_packages(&project)?;
            if json {
                return output::json(&wps);
            }
            if wps.is_empty() {
                println!("No work packages in {project}.");
            }
            for wp in wps {
                println!(
                    "{:20} {:11} {}",
                    wp.name,
                    wp.status.to_string(),
                    wp.title.unwrap_or_default()
                );
            }
        }
        WpCommand::Status { name, project, set } => {
            let project = project_for_cwd(&client, project)?;
            if let Some(status) = set {
                client.update_work_package(
                    &project,
                    &name,
                    &UpdateWorkPackage {
                        title: None,
                        status: Some(status),
                    },
                )?;
            }
            let detail = client.work_package(&project, &name)?;
            if json {
                return output::json(&detail);
            }
            let wp = &detail.work_package;
            println!("Work package {}/{}: {}", wp.project, wp.name, wp.status);
            if let Some(t) = &wp.title {
                println!("{t}");
            }
            if detail.sessions.is_empty() {
                println!("No review sessions.");
            }
            for s in &detail.sessions {
                let status = if s.status == rt_api::SessionStatus::Open {
                    "open"
                } else {
                    "closed"
                };
                println!(
                    "  {}  {status:6}  {}  ({})",
                    s.id,
                    s.title,
                    output::counts_line(s)
                );
            }
        }
    }
    Ok(())
}

pub fn project(cmd: ProjectCommand, json: bool) -> anyhow::Result<()> {
    let client = connect()?;
    match cmd {
        ProjectCommand::List => {
            let projects = client.projects()?;
            if json {
                return output::json(&projects);
            }
            if projects.is_empty() {
                println!("No projects. Run `rt init` in a repository to connect it.");
            }
            for p in projects {
                let repo = p.repo.map(|r| r.full_name).unwrap_or_default();
                println!("{:20} {repo}", p.name);
            }
        }
    }
    Ok(())
}

pub fn whoami(json: bool) -> anyhow::Result<()> {
    let endpoint = rt_client::discover()?;
    let client = crate::connect::connect()?;
    let me = client.whoami()?;
    if json {
        return output::json(&me);
    }
    let source = match &endpoint.source {
        rt_client::EndpointSource::Env => "RT_ENDPOINT".to_owned(),
        rt_client::EndpointSource::ConfigFile(p) => p.display().to_string(),
    };
    println!("Server: {} (from {source})", client.base());
    match me.kind {
        rt_api::PrincipalKind::Local => println!("The server signs no one in."),
        rt_api::PrincipalKind::User => println!(
            "Signed in as {}.",
            me.user.map(|u| u.login).unwrap_or_default()
        ),
        rt_api::PrincipalKind::Agent => {
            if let Some(t) = me.token {
                println!(
                    "Agent token `{}` ({}), for {}, created by {}.",
                    t.name,
                    format!("{:?}", t.role).to_lowercase(),
                    t.projects.join(", "),
                    t.creator.login
                );
            }
        }
    }
    Ok(())
}
