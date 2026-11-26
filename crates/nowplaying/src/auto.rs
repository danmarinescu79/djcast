//! The "automatic" now-playing source: reads rekordbox or Serato DJ, whichever is running
//! (rekordbox when both are), and follows a switch from one to the other on its own.

use crate::{LiveStatus, NowPlayingConfig, Source, Track, procs};
use anyhow::{Result, anyhow};
use std::time::{Duration, Instant};

/// How often to look at which DJ program runs.
const CHECK_EVERY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DjApp {
    Rekordbox,
    Serato,
}

impl DjApp {
    /// The program's name as matched against running processes.
    pub fn program(self) -> &'static str {
        match self {
            DjApp::Rekordbox => "rekordbox",
            DjApp::Serato => "Serato DJ",
        }
    }
}

/// The DJ program the automatic source reads now: rekordbox if it runs, else Serato DJ.
pub fn running_dj_app() -> Option<DjApp> {
    let names = procs::running();
    [DjApp::Rekordbox, DjApp::Serato].into_iter().find(|a| procs::program_running(&names, a.program()))
}

pub struct AutoSource {
    cfg: NowPlayingConfig,
    active: Option<(DjApp, Box<dyn Source>)>,
    checked: Option<Instant>,
    audible: bool,
}

impl AutoSource {
    pub fn new(cfg: &NowPlayingConfig) -> Self {
        Self { cfg: cfg.clone(), active: None, checked: None, audible: true }
    }

    fn follow_running_app(&mut self) -> Result<()> {
        if self.checked.is_some_and(|t| t.elapsed() < CHECK_EVERY) {
            return Ok(());
        }
        self.checked = Some(Instant::now());
        let app = running_dj_app();
        if app == self.active.as_ref().map(|(a, _)| *a) {
            return Ok(());
        }
        // Drop the previous source first: it releases its MIDI input and its reading thread.
        self.active = None;
        if let Some(app) = app {
            let mut cfg = self.cfg.clone();
            cfg.source = match app {
                DjApp::Rekordbox => crate::SourceKind::RekordboxLive,
                DjApp::Serato => crate::SourceKind::Serato,
            };
            let mut source = crate::create_source(&cfg)?.ok_or_else(|| anyhow!("no source for {app:?}"))?;
            source.set_audible(self.audible);
            log::info!("now playing: following {}", app.program());
            self.active = Some((app, source));
        }
        Ok(())
    }
}

impl Source for AutoSource {
    fn name(&self) -> &'static str {
        self.active.as_ref().map_or("auto", |(_, s)| s.name())
    }

    fn poll(&mut self) -> Result<Option<Track>> {
        self.follow_running_app()?;
        match &mut self.active {
            Some((_, source)) => source.poll(),
            None => Err(anyhow!("no DJ program is running (rekordbox or Serato DJ)")),
        }
    }

    fn set_audible(&mut self, audible: bool) {
        self.audible = audible;
        if let Some((_, s)) = &mut self.active {
            s.set_audible(audible);
        }
    }

    fn debug_snapshot(&self) -> Option<String> {
        self.active.as_ref().and_then(|(_, s)| s.debug_snapshot())
    }

    fn live_status(&self) -> Option<LiveStatus> {
        self.active.as_ref().and_then(|(_, s)| s.live_status())
    }
}
