//! Both render modes use this deterministic BCK playback controller.
//! It uses only the CPU. It does not read the wall clock or use the renderer.
//! It does not access the filesystem during each frame.
//!
//! The application calls these methods in this order:
//! 1. Update calls [`AnimationPlayer::advance`] with the elapsed seconds.
//! 2. The UI calls [`AnimationPlayer::apply`] once for each command.
//! 3. Draw calls [`AnimationPlayer::prepare_frame`] to get the pose for upload.
//!    This method evaluates at most one pose for each changed frame.
//!
//! Before it changes the selected clip, the controller reads, validates,
//! and prepares the candidate clip.
//! It also evaluates frame 0 before it changes the selected clip.
//! The controller evaluates each subsequent pose before it publishes that pose.
//! If evaluation fails, the controller keeps the last valid pose, frame, and clip.
//! It pauses playback and records a persistent diagnostic.

#![expect(unused)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Context, Result};
use gx::animation_manifest::{self as am, AnimationFormat, CatalogClip, ClipData, ClipIdentity};
use gx::model_manifest::Skeleton;

use crate::animation_pose::{Pose, PreparedClip, PreparedSkeleton};
use crate::animation_validation::{catalog_label, validate_catalog, validate_clip};

/// Animation frames per second at 1x speed (NTSC viewer convention).
pub const ANIMATION_FPS: f32 = 30.0;
pub const MIN_SPEED: f32 = 0.1;
pub const MAX_SPEED: f32 = 2.0;
pub const DEFAULT_SPEED: f32 = 1.0;
/// `Once` stops this many frames before the clip duration.
/// Matches Wind Waker's `J3DFrameCtrl::update`: `EMode_NONE` sets the frame
/// to `mEnd - 0.001f` when playback reaches the end.
/// See <https://github.com/zeldaret/tww/blob/main/src/JSystem/J3DGraphAnimator/J3DAnimation.cpp>.
pub const ONCE_END_MARGIN: f32 = 0.001;

/// Loop preference chosen in the UI. `Source` follows the clip's own metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoopPreference {
    #[default]
    Source,
    Repeat,
    Once,
}

/// Policy in force for the active clip after resolving `Source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopPolicy {
    Repeat,
    Once,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    /// Bind pose, a static clip, or a completed/parked `Once` clip.
    Stopped,
    Playing,
    /// Explicit pause, scrub, or a retained pose after an evaluation failure.
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Select(ClipIdentity),
    BindPose,
    Play,
    Pause,
    Restart,
    Scrub(f32),
    SetSpeed(f32),
    SetLoop(LoopPreference),
}

/// Commands that require an active clip with a positive duration.
#[derive(Clone, Copy, Debug)]
enum PlaybackCommand {
    Play,
    Restart,
    Scrub,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    Applied,
    /// The command would not change state (for example `Play` while playing).
    NoOp,
    /// The command is unavailable in the current state; nothing changed.
    Disabled(String),
    /// The command was rejected; the diagnostic holds the same message.
    Failed(String),
}

/// One searchable BCK catalog row. Identity is stable across filtering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRow {
    pub identity: ClipIdentity,
    /// `archive/member`, qualified by identity when member labels collide.
    pub label: String,
    search_key: String,
}

impl CatalogRow {
    /// Case-insensitive substring match over `archive/member`.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty() || self.search_key.contains(&query)
    }
}

struct ActiveClip {
    row: usize,
    generation: u64,
    duration: u16,
    /// Validated and prepared once at selection; frames only evaluate it.
    clip: PreparedClip,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PoseKey {
    Bind,
    Clip { generation: u64, frame: f32 },
}

pub struct AnimationPlayer {
    skeleton: Skeleton,
    prepared: Rc<PreparedSkeleton>,
    catalog_dir: PathBuf,
    entries: Vec<CatalogClip>,
    rows: Vec<CatalogRow>,
    catalog_error: Option<String>,
    active: Option<ActiveClip>,
    generation: u64,
    playback_state: PlaybackState,
    /// Requested frame. Equals `published_frame` once `prepare_frame` succeeds.
    frame: f32,
    /// Frame of the last successfully evaluated clip pose.
    published_frame: f32,
    speed: f32,
    preference: LoopPreference,
    diagnostic: Option<String>,
    /// A user-requested candidate may clear the diagnostic after publication.
    /// A later command failure cancels this permission, preserving command order.
    clear_on_publication: bool,
    bind: Pose,
    pose: Pose,
    pose_key: PoseKey,
    evaluations: u64,
    preparations: u64,
}

impl AnimationPlayer {
    /// Fails only when the skeleton cannot produce a bind pose. A missing or
    /// malformed catalog is not fatal: the player stays in bind pose, records
    /// [`Self::catalog_error`], and logs the problem on stderr.
    pub fn new(skeleton: &Skeleton, catalog_path: &Path) -> Result<Self> {
        let prepared = Rc::new(PreparedSkeleton::new(skeleton)?);
        let bind = prepared.bind_pose()?;
        let pose = prepared.bind_pose()?;
        let catalog_dir = catalog_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let (entries, catalog_error) = match load_catalog(catalog_path) {
            Ok(entries) => (entries, None),
            Err(error) => {
                let message = format!(
                    "BCK catalog {}: {error:#}; animation stays in bind pose. \
                     Run `just toon_link tww-assets` to extract and convert Link's assets.",
                    catalog_path.display()
                );
                eprintln!("toon_link: {message}");
                (Vec::new(), Some(message))
            }
        };
        let rows = catalog_rows(&entries);

        Ok(Self {
            skeleton: skeleton.clone(),
            prepared,
            catalog_dir,
            entries,
            rows,
            catalog_error,
            active: None,
            generation: 0,
            playback_state: PlaybackState::Stopped,
            frame: 0.0,
            published_frame: 0.0,
            speed: DEFAULT_SPEED,
            preference: LoopPreference::Source,
            diagnostic: None,
            clear_on_publication: false,
            bind,
            pose,
            pose_key: PoseKey::Bind,
            evaluations: 0,
            preparations: 0,
        })
    }

    // --- update ---------------------------------------------------------------

    /// Advances the clock while playing. Elapsed time while paused or stopped
    /// is discarded. Nonpositive or nonfinite elapsed time, or elapsed time
    /// whose target frame overflows, is ignored.
    pub fn advance(&mut self, elapsed_seconds: f32) {
        let usable_elapsed = elapsed_seconds.is_finite() && elapsed_seconds > 0.0;
        if self.playback_state != PlaybackState::Playing || !usable_elapsed {
            return;
        }

        let Some(policy) = self.effective_policy() else {
            return;
        };

        let duration = self.duration();
        let target = self.frame + elapsed_seconds * ANIMATION_FPS * self.speed;
        if !target.is_finite() {
            return;
        }

        // This target is automatic, not the pending user-requested candidate.
        self.clear_on_publication = false;
        match policy {
            LoopPolicy::Repeat => {
                self.frame = if target >= duration {
                    target.rem_euclid(duration)
                } else {
                    target
                };
            }
            LoopPolicy::Once => {
                let end = duration - ONCE_END_MARGIN;
                if target >= end {
                    self.frame = end;
                    self.playback_state = PlaybackState::Stopped;
                } else {
                    self.frame = target;
                }
            }
        }
    }

    // --- UI -------------------------------------------------------------------

    pub fn apply(&mut self, command: Command) -> CommandOutcome {
        match command {
            Command::Select(identity) => self.select(&identity),
            Command::BindPose => self.bind_pose(),
            Command::Play => self.play(),
            Command::Restart => self.restart(),
            Command::Scrub(frame) => self.scrub(frame),
            Command::Pause => self.pause(),
            Command::SetSpeed(speed) => self.set_speed(speed),
            Command::SetLoop(preference) => self.set_loop(preference),
        }
    }

    fn select(&mut self, identity: &ClipIdentity) -> CommandOutcome {
        let already_active = self
            .active
            .as_ref()
            .is_some_and(|active| self.rows[active.row].identity == *identity);
        if already_active {
            return CommandOutcome::NoOp;
        }

        let Some(row) = self.rows.iter().position(|row| row.identity == *identity) else {
            return self.fail(format!(
                "{}: not in the BCK catalog",
                identity_label(identity)
            ));
        };

        let (clip, pose) = match self.load(row) {
            Ok(loaded) => loaded,
            Err(error) => return self.fail(format!("{error:#}")),
        };

        self.preparations += 1;
        self.evaluations += 1;
        self.generation += 1;
        let duration = clip.clip().duration_frames;
        self.active = Some(ActiveClip {
            row,
            generation: self.generation,
            duration,
            clip,
        });
        self.frame = 0.0;
        self.published_frame = 0.0;
        self.playback_state = if duration > 0 {
            PlaybackState::Playing
        } else {
            PlaybackState::Stopped
        };
        self.pose = pose;
        self.pose_key = PoseKey::Clip {
            generation: self.generation,
            frame: 0.0,
        };
        self.diagnostic = None;
        self.clear_on_publication = false;

        CommandOutcome::Applied
    }

    /// Reads, validates, prepares and evaluates frame 0 without touching state.
    /// This is the only place clip data is read or validated; every later
    /// frame evaluates the returned [`PreparedClip`].
    fn load(&self, row: usize) -> Result<(PreparedClip, Pose)> {
        let entry = &self.entries[row];
        let label = catalog_label(entry);
        let path = self.catalog_dir.join(&entry.file);
        let json = fs::read_to_string(&path)
            .with_context(|| format!("{label}: read {}", path.display()))?;
        let document = am::read_clip(&json).with_context(|| label.clone())?;
        validate_clip(entry, &document, &self.skeleton)?;
        let ClipData::Bck(clip) = document.data else {
            unreachable!("validate_clip checked the format")
        };
        let clip = self.prepared.prepare_clip(Rc::new(clip))?;
        let pose = clip.evaluate(0.0).with_context(|| label)?;

        Ok((clip, pose))
    }

    fn bind_pose(&mut self) -> CommandOutcome {
        if self.active.is_none() {
            return CommandOutcome::NoOp;
        }

        self.active = None;
        self.playback_state = PlaybackState::Stopped;
        self.frame = 0.0;
        self.published_frame = 0.0;
        self.pose_key = PoseKey::Bind;
        self.diagnostic = None;
        self.clear_on_publication = false;

        CommandOutcome::Applied
    }

    fn play(&mut self) -> CommandOutcome {
        if let Err(outcome) = self.require_playback(PlaybackCommand::Play) {
            return outcome;
        }

        if self.playback_state == PlaybackState::Playing {
            return CommandOutcome::NoOp;
        }

        let duration = self.duration();
        match self.effective_policy() {
            Some(LoopPolicy::Once) if self.frame >= duration - ONCE_END_MARGIN => {
                // Park at the once endpoint; a completed clip stays stopped.
                let end = duration - ONCE_END_MARGIN;
                let changed = self.frame != end || self.playback_state != PlaybackState::Stopped;
                self.frame = end;
                self.playback_state = PlaybackState::Stopped;
                if changed {
                    self.clear_on_publication = true;
                    CommandOutcome::Applied
                } else {
                    CommandOutcome::NoOp
                }
            }
            Some(LoopPolicy::Repeat) if self.frame >= duration => {
                self.frame = 0.0;
                self.playback_state = PlaybackState::Playing;
                self.clear_on_publication = true;
                CommandOutcome::Applied
            }
            _ => {
                self.playback_state = PlaybackState::Playing;
                self.clear_on_publication = true;
                CommandOutcome::Applied
            }
        }
    }

    fn pause(&mut self) -> CommandOutcome {
        if self.active.is_none() {
            return CommandOutcome::Disabled("Pause: no active clip".into());
        }

        if self.playback_state != PlaybackState::Playing {
            return CommandOutcome::NoOp;
        }

        self.playback_state = PlaybackState::Paused;

        CommandOutcome::Applied
    }

    fn restart(&mut self) -> CommandOutcome {
        if let Err(outcome) = self.require_playback(PlaybackCommand::Restart) {
            return outcome;
        }

        self.frame = 0.0;
        self.playback_state = PlaybackState::Playing;
        self.clear_on_publication = true;

        CommandOutcome::Applied
    }

    fn scrub(&mut self, frame: f32) -> CommandOutcome {
        if let Err(outcome) = self.require_playback(PlaybackCommand::Scrub) {
            return outcome;
        }

        if !frame.is_finite() {
            return self.fail(format!("Scrub: nonfinite frame {frame}"));
        }

        self.frame = frame.clamp(0.0, self.duration());
        self.playback_state = PlaybackState::Paused;
        self.clear_on_publication = true;

        CommandOutcome::Applied
    }

    fn set_speed(&mut self, speed: f32) -> CommandOutcome {
        if !speed.is_finite() {
            return self.fail(format!("SetSpeed: nonfinite speed {speed}"));
        }

        let speed = speed.clamp(MIN_SPEED, MAX_SPEED);
        if speed == self.speed {
            return CommandOutcome::NoOp;
        }

        self.speed = speed;

        CommandOutcome::Applied
    }

    fn set_loop(&mut self, preference: LoopPreference) -> CommandOutcome {
        if preference == self.preference {
            return CommandOutcome::NoOp;
        }

        self.preference = preference;

        CommandOutcome::Applied
    }

    /// Play, Restart and Scrub need an active positive-duration clip.
    fn require_playback(&self, command: PlaybackCommand) -> Result<(), CommandOutcome> {
        let label = match command {
            PlaybackCommand::Play => "Play",
            PlaybackCommand::Restart => "Restart",
            PlaybackCommand::Scrub => "Scrub",
        };

        self.require_playback_with_label(label)
    }

    fn has_playback(&self) -> bool {
        self.require_playback_with_label("").is_ok()
    }

    fn require_playback_with_label(&self, command: &str) -> Result<(), CommandOutcome> {
        match &self.active {
            None => Err(CommandOutcome::Disabled(format!(
                "{command}: no active clip"
            ))),
            Some(active) if active.duration == 0 => Err(CommandOutcome::Disabled(format!(
                "{command}: {} is a static zero-duration clip",
                self.rows[active.row].label
            ))),
            Some(_) => Ok(()),
        }
    }

    fn fail(&mut self, message: String) -> CommandOutcome {
        self.clear_on_publication = false;
        self.diagnostic = Some(message.clone());

        CommandOutcome::Failed(message)
    }

    // --- draw -----------------------------------------------------------------

    /// Returns the pose for the current frame, evaluating only when the frame
    /// or clip changed since the last successful evaluation. On failure the
    /// previous valid pose and frame stay published, playback pauses, and the
    /// diagnostic names the clip and the failed target frame. Only successful
    /// publication of a user-requested candidate clears a diagnostic, including
    /// a known-valid cached pose. Automatic advancement never clears errors;
    /// a later command failure cancels an earlier request's clearing permission.
    pub fn prepare_frame(&mut self) -> &Pose {
        let clear_on_success = std::mem::take(&mut self.clear_on_publication);
        let Some(active) = &self.active else {
            return &self.bind;
        };

        let key = PoseKey::Clip {
            generation: active.generation,
            frame: self.frame,
        };
        if self.pose_key == key {
            if clear_on_success {
                self.diagnostic = None;
            }

            return &self.pose;
        }

        let target = self.frame;
        let result = active.clip.evaluate(target);
        let label = self.rows[active.row].label.clone();
        self.evaluations += 1;
        match result {
            Ok(pose) => {
                self.pose = pose;
                self.pose_key = key;
                self.published_frame = target;
                if clear_on_success {
                    self.diagnostic = None;
                }
            }
            Err(error) => {
                self.frame = self.published_frame;
                self.playback_state = PlaybackState::Paused;
                self.diagnostic = Some(format!(
                    "{label}: pose at frame {target} failed: {error:#}; \
                     paused at retained frame {}",
                    self.published_frame
                ));
            }
        }

        &self.pose
    }

    // --- accessors ------------------------------------------------------------

    /// Last validated palette. Call `prepare_frame` before publishing this frame.
    pub fn published_palette(&self) -> &[glam::Mat4] {
        if self.active.is_none() {
            return &self.bind.palette;
        }

        &self.pose.palette
    }

    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state
    }

    /// Requested frame; after a failed `prepare_frame` it is the retained frame.
    pub fn frame(&self) -> f32 {
        self.frame
    }

    pub fn duration_frames(&self) -> Option<u16> {
        self.active.as_ref().map(|active| active.duration)
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn loop_preference(&self) -> LoopPreference {
        self.preference
    }

    /// `None` without an active clip or for a static clip, where the
    /// preference has no effect.
    pub fn effective_policy(&self) -> Option<LoopPolicy> {
        let active = self.active.as_ref().filter(|active| active.duration > 0)?;
        Some(match self.preference {
            LoopPreference::Repeat => LoopPolicy::Repeat,
            LoopPreference::Once => LoopPolicy::Once,
            LoopPreference::Source => {
                if active.clip.clip().loop_attribute == 2 {
                    LoopPolicy::Repeat
                } else {
                    LoopPolicy::Once
                }
            }
        })
    }

    pub fn has_active_clip(&self) -> bool {
        self.active.is_some()
    }

    pub fn is_static(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.duration == 0)
    }

    pub fn selected_identity(&self) -> Option<&ClipIdentity> {
        self.active
            .as_ref()
            .map(|active| &self.rows[active.row].identity)
    }

    pub fn selected_label(&self) -> Option<&str> {
        self.active
            .as_ref()
            .map(|active| self.rows[active.row].label.as_str())
    }

    /// Persistent command or pose failure, cleared only after a successful
    /// user-requested pose publication (Select, BindPose, Play, Restart, Scrub); never by
    /// automatic advancement, SetSpeed, SetLoop, Pause or filtering.
    pub fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }

    /// Why the catalog is empty, when loading it failed.
    pub fn catalog_error(&self) -> Option<&str> {
        self.catalog_error.as_deref()
    }

    /// False while playing and at a completed Once endpoint, where `play`
    /// would be a no-op.
    pub fn can_play(&self) -> bool {
        let unavailable = !self.has_playback() || self.playback_state == PlaybackState::Playing;
        if unavailable {
            return false;
        }

        let completed_once = self.effective_policy() == Some(LoopPolicy::Once)
            && self.playback_state == PlaybackState::Stopped
            && self.frame >= self.duration() - ONCE_END_MARGIN;

        !completed_once
    }

    pub fn can_pause(&self) -> bool {
        self.playback_state == PlaybackState::Playing
    }

    pub fn can_restart(&self) -> bool {
        self.has_playback()
    }

    pub fn can_scrub(&self) -> bool {
        self.has_playback()
    }

    /// Every BCK row in catalog order. Filter with [`CatalogRow::matches`] or
    /// [`Self::search`]; neither changes playback.
    pub fn catalog_rows(&self) -> &[CatalogRow] {
        &self.rows
    }

    /// The debug UI cannot reach the player from the static editor hook, so it
    /// applies the same [`CatalogRow::matches`] to its own copy of the rows;
    /// the host tests check that both agree.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn search<'a>(&'a self, query: &str) -> impl Iterator<Item = &'a CatalogRow> + 'a {
        let query = query.trim().to_lowercase();
        self.rows.iter().filter(move |row| row.matches(&query))
    }

    /// Number of clip pose evaluations so far (selection and `prepare_frame`).
    /// A steady-state cost counter for the tests.
    #[cfg(test)]
    pub fn evaluation_count(&self) -> u64 {
        self.evaluations
    }

    /// Number of clips read, validated and prepared so far. Only successful
    /// selections count; `prepare_frame` never prepares.
    #[cfg(test)]
    pub fn preparation_count(&self) -> u64 {
        self.preparations
    }

    fn duration(&self) -> f32 {
        f32::from(self.active.as_ref().map_or(0, |active| active.duration))
    }
}

fn load_catalog(path: &Path) -> Result<Vec<CatalogClip>> {
    let json = fs::read_to_string(path).context("read")?;
    let catalog = am::read_catalog(&json)?;
    validate_catalog(&catalog)?;

    Ok(catalog
        .clips
        .into_iter()
        .filter(|entry| entry.format == AnimationFormat::Bck)
        .collect())
}

fn catalog_rows(entries: &[CatalogClip]) -> Vec<CatalogRow> {
    let mut label_counts: HashMap<String, usize> = HashMap::new();
    for entry in entries {
        *label_counts
            .entry(format!("{}/{}", entry.archive, entry.member))
            .or_default() += 1;
    }
    entries
        .iter()
        .map(|entry| {
            let base = format!("{}/{}", entry.archive, entry.member);
            let label = if label_counts[&base] > 1 {
                format!(
                    "{base} [entry={}, resource={}, sha256={}]",
                    entry.entry_index,
                    entry.resource_id,
                    &entry.sha256[..8]
                )
            } else {
                base.clone()
            };

            CatalogRow {
                identity: ClipIdentity {
                    archive: entry.archive.clone(),
                    member: entry.member.clone(),
                    entry_index: entry.entry_index,
                    resource_id: entry.resource_id,
                    sha256: entry.sha256.clone(),
                },
                label,
                search_key: base.to_lowercase(),
            }
        })
        .collect()
}

fn identity_label(identity: &ClipIdentity) -> String {
    format!(
        "{}/{} [entry={}, resource={}, sha256={}]",
        identity.archive,
        identity.member,
        identity.entry_index,
        identity.resource_id,
        identity.sha256
    )
}

/// Synthetic two-joint skeleton and on-disk catalog fixtures, shared with the
/// host tests in `main.rs`. No game assets.
#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::PathBuf;

    use gx::animation_manifest::{
        AnimationCatalog, AnimationClip, AnimationFormat, AxisSrt, BasMetadata, BckClip, BckJoint,
        CATALOG_VERSION, CLIP_VERSION, CatalogClip, ClipData, ClipIdentity, KeyF32, TrackF32,
        TrackI16,
    };
    use gx::model_manifest::{ScalingRule, Skeleton, SkeletonJoint};

    use super::{AnimationPlayer, catalog_rows};

    pub struct Fixture {
        pub dir: PathBuf,
        pub catalog: PathBuf,
        pub entries: Vec<CatalogClip>,
    }

    impl Fixture {
        pub fn identity(&self, index: usize) -> ClipIdentity {
            catalog_rows(&self.entries[index..=index])
                .remove(0)
                .identity
        }

        pub fn clip_path(&self, index: usize) -> PathBuf {
            self.dir.join(&self.entries[index].file)
        }

        pub fn player(&self) -> AnimationPlayer {
            AnimationPlayer::new(&skeleton(), &self.catalog).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("toon-link-bck-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("clips")).unwrap();

        dir
    }

    pub fn entry(
        index: usize,
        archive: &str,
        member: &str,
        format: AnimationFormat,
    ) -> CatalogClip {
        CatalogClip {
            archive: archive.into(),
            member: member.into(),
            entry_index: index as u32,
            resource_id: index as u16,
            format,
            sha256: format!("{index:0>64}"),
            file: format!("clips/{index}.json"),
        }
    }

    pub fn document(entry: &CatalogClip, clip: BckClip) -> AnimationClip {
        AnimationClip {
            version: CLIP_VERSION,
            identity: ClipIdentity {
                archive: entry.archive.clone(),
                member: entry.member.clone(),
                entry_index: entry.entry_index,
                resource_id: entry.resource_id,
                sha256: entry.sha256.clone(),
            },
            data: ClipData::Bck(clip),
        }
    }

    /// Writes a catalog whose BCK rows are `members` in order, each with its
    /// own clip document. `None` writes no document for that row.
    pub fn fixture(name: &str, members: &[(&str, Option<BckClip>)]) -> Fixture {
        let dir = temp_dir(name);
        let entries: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(index, (member, _))| entry(index, "LkAnm", member, AnimationFormat::Bck))
            .collect();
        for (entry, (_, clip)) in entries.iter().zip(members) {
            if let Some(clip) = clip {
                let json = serde_json::to_string(&document(entry, clip.clone())).unwrap();
                fs::write(dir.join(&entry.file), json).unwrap();
            }
        }
        let catalog = dir.join("catalog.json");
        let json = serde_json::to_string(&AnimationCatalog {
            version: CATALOG_VERSION,
            clips: entries.clone(),
        })
        .unwrap();
        fs::write(&catalog, json).unwrap();

        Fixture {
            dir,
            catalog,
            entries,
        }
    }

    pub fn skeleton() -> Skeleton {
        Skeleton {
            scaling_rule: ScalingRule::Maya,
            joints: [-1, 0]
                .iter()
                .enumerate()
                .map(|(index, &parent)| SkeletonJoint {
                    scale_compensate: false,
                    name: format!("joint{index}"),
                    parent,
                    t: [0.0; 3],
                    r_s16: [0; 3],
                    s: [1.0; 3],
                })
                .collect(),
        }
    }

    pub fn clip(duration_frames: u16, loop_attribute: u8) -> BckClip {
        BckClip {
            duration_frames,
            loop_attribute,
            rotation_decimal_shift: 0,
            joints: (0..2)
                .map(|ordinal| BckJoint {
                    ordinal,
                    axes: std::array::from_fn(|_| AxisSrt {
                        scale: TrackF32::Default,
                        rotation: TrackI16::Default,
                        translation: TrackF32::Default,
                    }),
                })
                .collect(),
            bas: BasMetadata {
                present: false,
                offset: 0,
                length: 0,
            },
        }
    }

    /// Root translation x moves linearly 0 -> duration over the clip, so the
    /// published pose exposes the evaluated frame.
    pub fn ramp_clip(duration_frames: u16, loop_attribute: u8) -> BckClip {
        let mut clip = clip(duration_frames, loop_attribute);
        clip.joints[0].axes[0].translation = TrackF32::Keyed {
            tangent_type: 0,
            keys: vec![
                KeyF32 {
                    time: 0.0,
                    value: 0.0,
                    tangent_in: 1.0,
                    tangent_out: 1.0,
                },
                KeyF32 {
                    time: f32::from(duration_frames),
                    value: f32::from(duration_frames),
                    tangent_in: 1.0,
                    tangent_out: 1.0,
                },
            ],
        };

        clip
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use glam::Mat4;
    use gx::animation_manifest::{AnimationCatalog, CATALOG_VERSION, KeyF32, TrackF32};

    const TOLERANCE: f32 = 1e-4;

    fn keyed_scale(tangent_out: f32, tangent_in: f32, end_value: f32) -> TrackF32 {
        TrackF32::Keyed {
            tangent_type: 1,
            keys: vec![
                KeyF32 {
                    time: 0.0,
                    value: 1.0,
                    tangent_in: 0.0,
                    tangent_out,
                },
                KeyF32 {
                    time: 10.0,
                    value: end_value,
                    tangent_in,
                    tangent_out: 0.0,
                },
            ],
        }
    }

    fn root_x(pose: &Pose) -> f32 {
        pose.model_space[0].w_axis.x
    }

    fn assert_frame(player: &AnimationPlayer, expected: f32) {
        let actual = player.frame();
        assert!(
            (actual - expected).abs() <= TOLERANCE,
            "frame {actual} != {expected}"
        );
    }

    fn tick(player: &mut AnimationPlayer, elapsed: f32) -> f32 {
        player.advance(elapsed);
        root_x(player.prepare_frame())
    }

    #[test]
    fn bck_catalog_lazy_search_identity() {
        let dir = temp_dir("lazy-search");
        let mut entries = vec![
            entry(0, "LkAnm", "bcks/Walk.bck", AnimationFormat::Bck),
            entry(1, "LkAnm", "bcks/run.bck", AnimationFormat::Bck),
            entry(2, "LkD00", "bcks/wait.bck", AnimationFormat::Bck),
            entry(3, "LkAnm", "btps/face.btp", AnimationFormat::Btp),
        ];
        entries.swap(1, 2);
        // Deliberate synthetic failures: write valid walk JSON, malformed run JSON,
        // and no wait file. These fixtures do not use extracted game assets.
        fs::write(
            dir.join(&entries[0].file),
            serde_json::to_string(&document(&entries[0], ramp_clip(20, 2))).unwrap(),
        )
        .unwrap();
        fs::write(dir.join(&entries[2].file), "{").unwrap();
        let catalog = dir.join("catalog.json");
        fs::write(
            &catalog,
            serde_json::to_string(&AnimationCatalog {
                version: CATALOG_VERSION,
                clips: entries.clone(),
            })
            .unwrap(),
        )
        .unwrap();
        let fixture = Fixture {
            dir,
            catalog,
            entries,
        };

        // Startup reads only the catalog: broken documents do not matter yet.
        let mut player = fixture.player();
        assert_eq!(player.catalog_error(), None);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert!(!player.has_active_clip());
        assert_eq!(player.evaluation_count(), 0);
        let labels: Vec<_> = player
            .catalog_rows()
            .iter()
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "LkAnm/bcks/Walk.bck",
                "LkD00/bcks/wait.bck",
                "LkAnm/bcks/run.bck"
            ]
        );
        let found: Vec<_> = player
            .search(" wA ")
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(found, ["LkAnm/bcks/Walk.bck", "LkD00/bcks/wait.bck"]);
        let found: Vec<_> = player
            .search("lkd00")
            .map(|row| row.label.as_str())
            .collect();
        assert_eq!(found, ["LkD00/bcks/wait.bck"]);
        assert_eq!(player.search("").count(), 3);
        assert_eq!(player.search("btp").count(), 0);
        assert_eq!(
            player.catalog_rows()[1].identity,
            fixture.identity(1),
            "identity is the catalog identity, not a row index"
        );

        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );
        assert_eq!(player.selected_identity(), Some(&fixture.identity(0)));
        assert_eq!(player.selected_label(), Some("LkAnm/bcks/Walk.bck"));
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        assert_eq!(player.evaluation_count(), 1);

        let CommandOutcome::Failed(message) = player.apply(Command::Select(fixture.identity(1)))
        else {
            panic!("missing document must fail")
        };
        assert!(message.contains("LkD00/bcks/wait.bck"), "{message}");
        assert!(message.contains("read"), "{message}");
        assert_eq!(player.diagnostic(), Some(message.as_str()));
        assert_eq!(player.selected_identity(), Some(&fixture.identity(0)));

        let CommandOutcome::Failed(message) = player.apply(Command::Select(fixture.identity(2)))
        else {
            panic!("malformed document must fail")
        };
        assert!(message.contains("LkAnm/bcks/run.bck"), "{message}");
        assert_eq!(player.selected_identity(), Some(&fixture.identity(0)));

        let unknown = ClipIdentity {
            member: "bcks/nope.bck".into(),
            ..fixture.identity(0)
        };
        let CommandOutcome::Failed(message) = player.apply(Command::Select(unknown)) else {
            panic!("unknown identity must fail")
        };
        assert!(message.contains("bcks/nope.bck"), "{message}");
        assert_eq!(player.evaluation_count(), 1);
    }

    #[test]
    fn bck_missing_catalog_bind_fallback() {
        // The fixture's own catalog is unused; it only owns the directory.
        let fixture = fixture("missing-catalog", &[]);
        let dir = &fixture.dir;
        let bind = PreparedSkeleton::new(&skeleton())
            .unwrap()
            .bind_pose()
            .unwrap();
        let cases: [(&str, Option<&str>, &str); 4] = [
            ("absent", None, "absent.json"),
            ("malformed", Some("{"), "malformed.json"),
            (
                "version",
                Some(r#"{"version":7,"clips":[]}"#),
                "version.json",
            ),
            (
                "duplicate",
                Some(
                    &serde_json::to_string(&AnimationCatalog {
                        version: CATALOG_VERSION,
                        clips: vec![
                            entry(0, "LkAnm", "bcks/a.bck", AnimationFormat::Bck),
                            entry(0, "LkAnm", "bcks/a.bck", AnimationFormat::Bck),
                        ],
                    })
                    .unwrap(),
                ),
                "duplicate.json",
            ),
        ];
        for (name, contents, file) in cases {
            let path = dir.join(file);
            if let Some(contents) = contents {
                fs::write(&path, contents).unwrap();
            }
            let mut player = AnimationPlayer::new(&skeleton(), &path).unwrap();
            let error = player
                .catalog_error()
                .unwrap_or_else(|| panic!("{name}: expected catalog error"));
            assert!(error.contains(file), "{name}: {error}");
            assert!(error.contains("bind pose"), "{name}: {error}");
            assert!(
                error.contains("just toon_link tww-assets"),
                "{name}: {error}"
            );
            assert!(player.catalog_rows().is_empty());
            assert_eq!(player.playback_state(), PlaybackState::Stopped);
            assert!(matches!(
                player.apply(Command::Play),
                CommandOutcome::Disabled(_)
            ));
            assert!(matches!(
                player.apply(Command::Restart),
                CommandOutcome::Disabled(_)
            ));
            assert!(matches!(
                player.apply(Command::Scrub(3.0)),
                CommandOutcome::Disabled(_)
            ));
            assert_eq!(player.apply(Command::BindPose), CommandOutcome::NoOp);
            assert!(!player.can_play() && !player.can_restart() && !player.can_scrub());
            player.advance(1.0);
            assert_eq!(player.prepare_frame().palette, bind.palette);
            assert_eq!(player.prepare_frame().palette, vec![Mat4::IDENTITY; 2]);
            assert_eq!(player.evaluation_count(), 0);
            assert_eq!(player.diagnostic(), None);
        }
    }

    #[test]
    fn bck_command_first_frame_once() {
        let fixture = fixture("first-frame", &[("bcks/walk.bck", Some(ramp_clip(20, 2)))]);
        let mut player = fixture.player();
        let walk = fixture.identity(0);

        // update -> UI -> draw: the selection is visible on the first draw.
        player.advance(1.0);
        assert_eq!(player.apply(Command::Select(walk)), CommandOutcome::Applied);
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert_eq!(player.evaluation_count(), 1);

        player.advance(0.1);
        assert_eq!(player.apply(Command::Scrub(7.5)), CommandOutcome::Applied);
        assert_eq!(player.playback_state(), PlaybackState::Paused);
        assert_eq!(root_x(player.prepare_frame()), 7.5);
        assert_eq!(player.evaluation_count(), 2);
        // The same command is not re-applied: later frames advance from it.
        assert_eq!(tick(&mut player, 1.0), 7.5);
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        assert!((tick(&mut player, 0.1) - 10.5).abs() <= TOLERANCE);

        player.advance(0.1);
        assert_eq!(player.apply(Command::Restart), CommandOutcome::Applied);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert!((tick(&mut player, 0.1) - 3.0).abs() <= TOLERANCE);

        player.advance(0.1);
        assert_eq!(player.apply(Command::BindPose), CommandOutcome::Applied);
        assert_eq!(player.prepare_frame().palette, vec![Mat4::IDENTITY; 2]);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert_eq!(player.selected_identity(), None);
    }

    #[test]
    fn bck_pause_resume_no_catchup() {
        let fixture = fixture(
            "no-catchup",
            &[
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
                ("bcks/run.bck", Some(ramp_clip(40, 2))),
            ],
        );
        let mut player = fixture.player();
        player.apply(Command::Select(fixture.identity(0)));
        assert!((tick(&mut player, 0.1) - 3.0).abs() <= TOLERANCE);
        assert_eq!(player.apply(Command::Pause), CommandOutcome::Applied);
        assert_eq!(player.apply(Command::Pause), CommandOutcome::NoOp);
        let paused = root_x(player.prepare_frame());
        assert_eq!(tick(&mut player, 5.0), paused);
        assert_eq!(tick(&mut player, 5.0), paused);
        assert_frame(&player, 3.0);
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        assert_eq!(player.apply(Command::Play), CommandOutcome::NoOp);
        assert!((tick(&mut player, 0.1) - 6.0).abs() <= TOLERANCE);
        assert_frame(&player, 6.0);

        // Time spent in bind pose is not credited to the next clip.
        player.apply(Command::BindPose);
        player.advance(5.0);
        assert_eq!(
            player.apply(Command::Select(fixture.identity(1))),
            CommandOutcome::Applied
        );
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert!((tick(&mut player, 0.1) - 3.0).abs() <= TOLERANCE);

        // Nor is the previous clip's position.
        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );
        assert_frame(&player, 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        player.advance(0.0);
        player.advance(-1.0);
        player.advance(f32::NAN);
        assert_frame(&player, 0.0);
    }

    fn playing_repeat_fixture(name: &str) -> (Fixture, AnimationPlayer) {
        let fixture = fixture(name, &[("bcks/repeat.bck", Some(ramp_clip(20, 2)))]);
        let mut player = fixture.player();
        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );

        (fixture, player)
    }

    #[test]
    fn bck_repeat_wrap_preserves_overshoot() {
        let (_fixture, mut player) = playing_repeat_fixture("bck_repeat_wrap_preserves_overshoot");
        assert_eq!(player.effective_policy(), Some(LoopPolicy::Repeat));
        assert_eq!(player.duration_frames(), Some(20));

        // Repeat: wrap preserving overshoot, including multiple cycles and
        // an exact landing on the duration.
        player.apply(Command::Scrub(18.0));
        player.apply(Command::Play);
        assert!((tick(&mut player, 5.0 / 30.0) - 3.0).abs() <= TOLERANCE);
        player.apply(Command::Scrub(0.0));
        player.apply(Command::Play);
        assert!((tick(&mut player, 45.0 / 30.0) - 5.0).abs() <= TOLERANCE);
        player.apply(Command::Scrub(10.0));
        player.apply(Command::Play);
        assert_eq!(tick(&mut player, 10.0 / 30.0), 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
    }

    #[test]
    fn bck_elapsed_schedules_reach_the_same_frame() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_elapsed_schedules_reach_the_same_frame");
        // Alternate frame schedules reach the same frame.
        player.apply(Command::Scrub(0.0));
        player.apply(Command::Play);
        for _ in 0..6 {
            player.advance(1.0 / 60.0);
        }
        assert_frame(&player, 3.0);
        player.apply(Command::Scrub(0.0));
        player.apply(Command::Play);
        for _ in 0..3 {
            player.advance(1.0 / 30.0);
        }
        assert_frame(&player, 3.0);
        player.apply(Command::Scrub(0.0));
        player.apply(Command::Play);
        player.advance(0.1);
        assert_frame(&player, 3.0);
    }

    #[test]
    fn bck_overflowing_elapsed_time_keeps_frame_and_playback_state() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_overflowing_elapsed_time_keeps_frame_and_playback_state");
        player.advance(0.1);
        // An elapsed time whose target frame overflows is ignored, never stored.
        player.advance(f32::MAX);
        assert_frame(&player, 3.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
    }

    #[test]
    fn bck_speed_scales_elapsed_time_and_clamps_to_supported_range() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_speed_scales_elapsed_time_and_clamps_to_supported_range");
        // Speed scales the clock and is clamped to the supported range.
        assert_eq!(
            player.apply(Command::SetSpeed(5.0)),
            CommandOutcome::Applied
        );
        assert_eq!(player.speed(), MAX_SPEED);
        assert_eq!(player.apply(Command::SetSpeed(2.0)), CommandOutcome::NoOp);
        player.apply(Command::Scrub(0.0));
        player.apply(Command::Play);
        player.advance(0.1);
        assert_frame(&player, 6.0);
        assert_eq!(
            player.apply(Command::SetSpeed(0.0)),
            CommandOutcome::Applied
        );
        assert_eq!(player.speed(), MIN_SPEED);
        assert!(matches!(
            player.apply(Command::SetSpeed(f32::INFINITY)),
            CommandOutcome::Failed(_)
        ));
        assert_eq!(player.speed(), MIN_SPEED);
        player.apply(Command::SetSpeed(1.0));
    }

    #[test]
    fn bck_scrub_clamps_to_closed_duration_interval_and_rejects_nan() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_scrub_clamps_to_closed_duration_interval_and_rejects_nan");
        // Exact-duration scrub lands on the closed interval endpoint.
        assert_eq!(player.apply(Command::Scrub(20.0)), CommandOutcome::Applied);
        assert_eq!(player.playback_state(), PlaybackState::Paused);
        assert_eq!(root_x(player.prepare_frame()), 20.0);
        player.apply(Command::Scrub(25.0));
        assert_frame(&player, 20.0);
        player.apply(Command::Scrub(-1.0));
        assert_frame(&player, 0.0);
        assert!(matches!(
            player.apply(Command::Scrub(f32::NAN)),
            CommandOutcome::Failed(_)
        ));
        assert_frame(&player, 0.0);
    }

    #[test]
    fn bck_repeat_play_at_duration_wraps_to_zero() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_repeat_play_at_duration_wraps_to_zero");
        // Play at duration wraps for Repeat.
        player.apply(Command::Scrub(20.0));
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        assert_frame(&player, 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
    }

    #[test]
    fn bck_once_stops_before_duration_and_restart_resumes() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_once_stops_before_duration_and_restart_resumes");
        // Once via override: stops at duration - 0.001.
        assert_eq!(
            player.apply(Command::SetLoop(LoopPreference::Once)),
            CommandOutcome::Applied
        );
        assert_eq!(player.effective_policy(), Some(LoopPolicy::Once));
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        player.apply(Command::Scrub(18.0));
        player.apply(Command::Play);
        player.advance(5.0 / 30.0);
        assert_frame(&player, 19.999);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert!((root_x(player.prepare_frame()) - 19.999).abs() <= TOLERANCE);
        // Play at a completed once endpoint remains stopped, and the UI must
        // not offer it; Restart begins again.
        assert!(!player.can_play());
        assert!(player.can_restart());
        assert_eq!(player.apply(Command::Play), CommandOutcome::NoOp);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert_frame(&player, 19.999);
        assert_eq!(player.apply(Command::Restart), CommandOutcome::Applied);
        assert_frame(&player, 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
    }

    #[test]
    fn bck_once_play_at_duration_parks_before_endpoint() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_once_play_at_duration_parks_before_endpoint");
        player.apply(Command::SetLoop(LoopPreference::Once));
        // Play at duration parks for Once.
        player.apply(Command::Scrub(20.0));
        assert!(player.can_play());
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        assert_frame(&player, 19.999);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert!(!player.can_play());
    }

    #[test]
    fn bck_loop_policy_change_applies_on_next_advance() {
        let (_fixture, mut player) =
            playing_repeat_fixture("bck_loop_policy_change_applies_on_next_advance");
        player.apply(Command::SetLoop(LoopPreference::Once));
        // Policy changes apply at the next advance, not immediately.
        player.apply(Command::Scrub(18.0));
        player.apply(Command::Play);
        player.apply(Command::SetLoop(LoopPreference::Repeat));
        assert_frame(&player, 18.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        player.advance(5.0 / 30.0);
        assert_frame(&player, 3.0);
    }

    #[test]
    fn bck_source_policy_follows_clip_and_preferences_survive_selection() {
        let fixture = fixture(
            "source-policy",
            &[
                ("bcks/repeat.bck", Some(ramp_clip(20, 2))),
                ("bcks/once.bck", Some(ramp_clip(20, 0))),
            ],
        );
        let mut player = fixture.player();
        player.apply(Command::Select(fixture.identity(0)));
        // Source honors the clip's loop attribute (0 => Once, 2 => Repeat),
        // and the preference survives clip changes and bind pose.
        player.apply(Command::SetLoop(LoopPreference::Source));
        player.apply(Command::SetSpeed(0.5));
        player.apply(Command::BindPose);
        assert_eq!(player.effective_policy(), None);
        player.apply(Command::Select(fixture.identity(1)));
        assert_eq!(player.loop_preference(), LoopPreference::Source);
        assert_eq!(player.speed(), 0.5);
        assert_eq!(player.effective_policy(), Some(LoopPolicy::Once));
        player.advance(2.0);
        assert_frame(&player, 19.999);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        player.apply(Command::SetLoop(LoopPreference::Repeat));
        player.apply(Command::Select(fixture.identity(0)));
        assert_eq!(player.loop_preference(), LoopPreference::Repeat);
        assert_eq!(player.effective_policy(), Some(LoopPolicy::Repeat));
    }

    #[test]
    fn bck_static_clip_controls() {
        let mut static_clip = clip(0, 2);
        static_clip.joints[0].axes[0].translation = TrackF32::Constant { value: 4.0 };
        let fixture = fixture(
            "static",
            &[
                ("bcks/rise.bck", Some(static_clip)),
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
            ],
        );
        let mut player = fixture.player();
        player.apply(Command::SetSpeed(1.5));
        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );
        assert!(player.is_static());
        assert!(player.has_active_clip());
        assert_eq!(player.duration_frames(), Some(0));
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert_eq!(player.effective_policy(), None);
        assert!(!player.can_play() && !player.can_restart() && !player.can_scrub());
        assert_eq!(
            root_x(player.prepare_frame()),
            4.0,
            "frame 0, not bind pose"
        );
        assert_eq!(player.evaluation_count(), 1);

        for command in [Command::Play, Command::Restart, Command::Scrub(0.0)] {
            let CommandOutcome::Disabled(reason) = player.apply(command.clone()) else {
                panic!("{command:?} must be disabled for a static clip")
            };
            assert!(reason.contains("static"), "{reason}");
            assert!(reason.contains("bcks/rise.bck"), "{reason}");
            assert_eq!(player.playback_state(), PlaybackState::Stopped);
            assert_frame(&player, 0.0);
        }
        assert_eq!(player.apply(Command::Pause), CommandOutcome::NoOp);
        assert_eq!(
            player.apply(Command::SetLoop(LoopPreference::Once)),
            CommandOutcome::Applied
        );
        assert_eq!(player.effective_policy(), None);
        for _ in 0..10 {
            player.advance(1.0);
            assert_eq!(root_x(player.prepare_frame()), 4.0);
        }
        assert_frame(&player, 0.0);
        assert_eq!(player.evaluation_count(), 1);
        assert_eq!(player.diagnostic(), None);

        // Preferences survive; the next positive-duration clip plays normally.
        player.apply(Command::Select(fixture.identity(1)));
        assert!(!player.is_static());
        assert_eq!(player.speed(), 1.5);
        assert_eq!(player.loop_preference(), LoopPreference::Once);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        assert!((tick(&mut player, 0.1) - 4.5).abs() <= TOLERANCE);
        assert_eq!(player.apply(Command::BindPose), CommandOutcome::Applied);
        assert_eq!(player.apply(Command::BindPose), CommandOutcome::NoOp);
    }

    #[test]
    fn bck_completed_once_to_repeat_play() {
        let fixture = fixture("once-repeat", &[("bcks/once.bck", Some(ramp_clip(20, 0)))]);
        let mut player = fixture.player();
        player.apply(Command::Select(fixture.identity(0)));
        player.advance(1.0);
        assert_frame(&player, 19.999);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert!(!player.can_play());
        assert_eq!(player.apply(Command::Play), CommandOutcome::NoOp);

        // Completion depends on the current effective policy: switching to
        // Repeat keeps the retained frame and the stopped state until Play,
        // and re-enables Play consistently with its outcome.
        assert_eq!(
            player.apply(Command::SetLoop(LoopPreference::Repeat)),
            CommandOutcome::Applied
        );
        assert_frame(&player, 19.999);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        assert!(player.can_play());
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        assert_frame(&player, 19.999);
        assert!((tick(&mut player, 0.1) - 2.999).abs() <= TOLERANCE);
        assert_eq!(player.playback_state(), PlaybackState::Playing);

        // Back to an effective Once: the same endpoint parks again, and
        // Source resolving to Once behaves the same.
        player.apply(Command::SetLoop(LoopPreference::Once));
        player.advance(1.0);
        assert_frame(&player, 19.999);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        player.apply(Command::SetLoop(LoopPreference::Source));
        assert!(!player.can_play());
        assert_eq!(player.apply(Command::Play), CommandOutcome::NoOp);
        assert_eq!(player.playback_state(), PlaybackState::Stopped);
        player.apply(Command::SetLoop(LoopPreference::Repeat));
        assert!(player.can_play());
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        assert!(!player.can_play());
    }

    #[test]
    fn bck_validation_atomic_selection() {
        let fixture = fixture(
            "atomic",
            &[
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
                ("bcks/badloop.bck", Some(ramp_clip(20, 1))),
                ("bcks/mismatch.bck", Some(ramp_clip(20, 2))),
                ("bcks/missing.bck", None),
                ("bcks/overflow.bck", Some(ramp_clip(20, 2))),
            ],
        );
        // Identity mismatch: the document carries another entry's identity.
        fs::write(
            fixture.clip_path(2),
            serde_json::to_string(&document(&fixture.entries[0], ramp_clip(20, 2))).unwrap(),
        )
        .unwrap();
        // Frame-0 hierarchy overflow: valid structure, nonfinite initial pose.
        let mut overflow = ramp_clip(20, 2);
        for joint in &mut overflow.joints {
            joint.axes[0].scale = TrackF32::Constant { value: 1e30 };
        }
        fs::write(
            fixture.clip_path(4),
            serde_json::to_string(&document(&fixture.entries[4], overflow)).unwrap(),
        )
        .unwrap();
        let failures = [
            (1, "loop_attribute"),
            (2, "clip.identity"),
            (3, "read"),
            (4, "hierarchy"),
        ];

        for playing in [false, true] {
            let mut player = fixture.player();
            player.apply(Command::SetSpeed(0.5));
            player.apply(Command::SetLoop(LoopPreference::Once));
            player.apply(Command::Select(fixture.identity(0)));
            player.apply(Command::Scrub(5.0));
            if playing {
                player.apply(Command::Play);
            }
            let before = root_x(player.prepare_frame());
            let evaluations = player.evaluation_count();
            for (index, field) in failures {
                let CommandOutcome::Failed(message) =
                    player.apply(Command::Select(fixture.identity(index)))
                else {
                    panic!("selection {index} must fail")
                };
                assert!(message.contains(field), "{message}");
                assert!(
                    message.contains(&fixture.entries[index].member),
                    "{message}"
                );
                assert_eq!(player.diagnostic(), Some(message.as_str()));
                assert_eq!(player.selected_identity(), Some(&fixture.identity(0)));
                assert_frame(&player, 5.0);
                assert_eq!(
                    player.playback_state(),
                    if playing {
                        PlaybackState::Playing
                    } else {
                        PlaybackState::Paused
                    }
                );
                assert_eq!(player.speed(), 0.5);
                assert_eq!(player.loop_preference(), LoopPreference::Once);
                assert_eq!(root_x(player.prepare_frame()), before);
                assert_eq!(player.evaluation_count(), evaluations);
            }
            // The valid clip keeps advancing normally afterwards. Automatic
            // advancement is not a user action, so the error stays visible
            // whether or not a new pose was drawn; an applied Scrub clears it.
            let after = tick(&mut player, 0.2);
            if playing {
                assert!((after - 8.0).abs() <= TOLERANCE);
            } else {
                assert_eq!(after, before);
            }
            assert!(player.diagnostic().is_some());
            assert_eq!(player.apply(Command::Scrub(6.0)), CommandOutcome::Applied);
            assert_eq!(root_x(player.prepare_frame()), 6.0);
            assert_eq!(player.diagnostic(), None);
        }
    }

    #[test]
    fn bck_command_failure_diagnostic_persists_across_draws() {
        let mut hermite = ramp_clip(20, 2);
        hermite.joints[0].axes[1].scale = keyed_scale(f32::MAX, -f32::MAX, 1.0);
        let fixture = fixture(
            "diagnostic-lifetime",
            &[
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
                ("bcks/missing.bck", None),
                ("bcks/hermite.bck", Some(hermite)),
            ],
        );
        let mut player = fixture.player();
        player.apply(Command::Select(fixture.identity(0)));
        assert_eq!(player.playback_state(), PlaybackState::Playing);

        // A failed Select while playing survives many successful draws and
        // every non-pose command; only an applied pose-changing command clears it.
        let CommandOutcome::Failed(message) = player.apply(Command::Select(fixture.identity(1)))
        else {
            panic!("missing clip must fail")
        };
        let mut last = root_x(player.prepare_frame());
        for _ in 0..10 {
            let next = tick(&mut player, 1.0 / 60.0);
            assert!(next > last, "{next} must advance past {last}");
            last = next;
            assert_eq!(player.diagnostic(), Some(message.as_str()));
        }
        assert_eq!(
            player.apply(Command::SetSpeed(2.0)),
            CommandOutcome::Applied
        );
        assert_eq!(
            player.apply(Command::SetLoop(LoopPreference::Once)),
            CommandOutcome::Applied
        );
        assert_eq!(player.apply(Command::Pause), CommandOutcome::Applied);
        assert_eq!(player.apply(Command::Play), CommandOutcome::Applied);
        // Play clears only after publishing its known-valid cached candidate.
        assert!(player.diagnostic().is_some());
        player.prepare_frame();
        assert_eq!(player.diagnostic(), None);
        assert!(matches!(
            player.apply(Command::Select(fixture.identity(1))),
            CommandOutcome::Failed(_)
        ));
        assert!(player.diagnostic().is_some());
        assert_eq!(player.apply(Command::Pause), CommandOutcome::Applied);
        assert!(player.diagnostic().is_some());
        assert_eq!(player.apply(Command::Scrub(4.0)), CommandOutcome::Applied);
        assert!(player.diagnostic().is_some());
        assert!((root_x(player.prepare_frame()) - 4.0).abs() <= TOLERANCE);
        assert_eq!(player.diagnostic(), None);

        // A pose failure survives repeated draws while paused and is cleared
        // by an applied Restart, not by the cached redraws in between.
        player.apply(Command::SetLoop(LoopPreference::Repeat));
        assert_eq!(
            player.apply(Command::Select(fixture.identity(2))),
            CommandOutcome::Applied
        );
        assert_eq!(player.apply(Command::Scrub(3.0)), CommandOutcome::Applied);
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Paused);
        let diagnostic = player.diagnostic().unwrap().to_owned();
        assert!(diagnostic.contains("frame 3"), "{diagnostic}");
        let evaluations = player.evaluation_count();
        for _ in 0..10 {
            assert_eq!(tick(&mut player, 1.0 / 60.0), 0.0);
            assert_eq!(player.diagnostic(), Some(diagnostic.as_str()));
        }
        assert_eq!(player.evaluation_count(), evaluations);
        assert_eq!(player.apply(Command::Restart), CommandOutcome::Applied);
        assert!(player.diagnostic().is_some());
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert_eq!(player.diagnostic(), None);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
    }

    #[test]
    fn bck_late_pose_failure_retains_and_pauses() {
        // Interior Hermite overflow: frame 0 and frame >= 10 clamp to keys,
        // every interior frame scales a tangent to infinity.
        let mut hermite = ramp_clip(20, 2);
        hermite.joints[0].axes[1].scale = keyed_scale(f32::MAX, -f32::MAX, 1.0);
        // Interior hierarchy overflow: finite samples whose parent*child product
        // overflows after frame 0.
        let mut hierarchy = ramp_clip(20, 2);
        for joint in &mut hierarchy.joints {
            joint.axes[1].scale = keyed_scale(0.0, 0.0, 1e30);
        }
        let fixture = fixture(
            "late-failure",
            &[
                ("bcks/hermite.bck", Some(hermite)),
                ("bcks/hierarchy.bck", Some(hierarchy)),
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
            ],
        );
        let mut player = fixture.player();
        player.apply(Command::SetSpeed(2.0));
        player.apply(Command::SetLoop(LoopPreference::Once));

        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        player.advance(0.05);
        assert_frame(&player, 3.0);
        assert_eq!(root_x(player.prepare_frame()), 0.0, "last valid pose");
        assert_frame(&player, 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Paused);
        assert_eq!(player.selected_identity(), Some(&fixture.identity(0)));
        assert_eq!(player.speed(), 2.0);
        assert_eq!(player.loop_preference(), LoopPreference::Once);
        let diagnostic = player.diagnostic().unwrap().to_owned();
        assert!(
            diagnostic.contains("LkAnm/bcks/hermite.bck"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("frame 3"), "{diagnostic}");
        assert!(diagnostic.contains("Hermite"), "{diagnostic}");
        // The diagnostic persists across mode-neutral queries and repeated draws.
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert_eq!(player.diagnostic(), Some(diagnostic.as_str()));
        assert_eq!(player.evaluation_count(), 2);
        // Play retries the same target and fails the same way.
        player.apply(Command::Play);
        player.advance(0.05);
        player.prepare_frame();
        assert_eq!(player.playback_state(), PlaybackState::Paused);
        assert_frame(&player, 0.0);
        // A safe scrub succeeds and clears the error; Restart stays available.
        assert_eq!(player.apply(Command::Scrub(10.0)), CommandOutcome::Applied);
        assert_eq!(root_x(player.prepare_frame()), 10.0);
        assert_eq!(player.diagnostic(), None);
        assert_frame(&player, 10.0);
        assert_eq!(player.apply(Command::Restart), CommandOutcome::Applied);
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Playing);

        // Hierarchy overflow discovered by a scrub: the scrub target is
        // dropped, the retained frame stays published.
        assert_eq!(
            player.apply(Command::Select(fixture.identity(1))),
            CommandOutcome::Applied
        );
        player.apply(Command::Scrub(5.0));
        assert_eq!(root_x(player.prepare_frame()), 0.0);
        assert_frame(&player, 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Paused);
        let diagnostic = player.diagnostic().unwrap().to_owned();
        assert!(
            diagnostic.contains("LkAnm/bcks/hierarchy.bck"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("frame 5"), "{diagnostic}");
        assert!(diagnostic.contains("hierarchy"), "{diagnostic}");

        // A different clip and bind pose remain available.
        assert_eq!(
            player.apply(Command::Select(fixture.identity(2))),
            CommandOutcome::Applied
        );
        assert_eq!(player.diagnostic(), None);
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        player.apply(Command::Select(fixture.identity(1)));
        player.apply(Command::Scrub(5.0));
        player.prepare_frame();
        assert!(player.diagnostic().is_some());
        assert_eq!(player.apply(Command::BindPose), CommandOutcome::Applied);
        assert_eq!(player.diagnostic(), None);
        assert_eq!(player.prepare_frame().palette, vec![Mat4::IDENTITY; 2]);
    }

    #[test]
    fn bck_deferred_diagnostic_success_and_command_order() {
        let mut unsafe_clip = ramp_clip(20, 2);
        unsafe_clip.joints[0].axes[0].scale = keyed_scale(f32::MAX, -f32::MAX, 1.0);
        let fixture = fixture(
            "deferred-errors",
            &[("unsafe.bck", Some(unsafe_clip)), ("missing.bck", None)],
        );
        let mut player = fixture.player();
        player.apply(Command::Select(fixture.identity(0)));
        player.apply(Command::Scrub(3.0));
        player.prepare_frame();
        let error = player.diagnostic().unwrap().to_owned();
        let retained = player.published_palette().to_vec();
        player.apply(Command::Scrub(3.0));
        assert_eq!(player.diagnostic(), Some(error.as_str()));
        player.prepare_frame();
        assert!(player.diagnostic().is_some());
        assert_eq!(player.published_palette(), retained);
        assert_frame(&player, 0.0);
        assert_eq!(player.playback_state(), PlaybackState::Paused);

        // A safe candidate cannot erase a later failed selection.
        player.apply(Command::Scrub(10.0));
        player.apply(Command::Select(fixture.identity(1)));
        let newer = player.diagnostic().unwrap().to_owned();
        player.prepare_frame();
        assert_frame(&player, 10.0);
        assert_eq!(player.diagnostic(), Some(newer.as_str()));
        let evaluations = player.evaluation_count();
        player.apply(Command::Scrub(10.0));
        assert_eq!(player.diagnostic(), Some(newer.as_str()));
        player.prepare_frame();
        assert_eq!(player.diagnostic(), None);
        assert_eq!(
            player.evaluation_count(),
            evaluations,
            "cached safe request"
        );

        player.apply(Command::Select(fixture.identity(1)));
        player.apply(Command::Scrub(0.0));
        assert!(player.diagnostic().is_some());
        player.prepare_frame();
        assert_eq!(player.diagnostic(), None);
        assert_frame(&player, 0.0);
    }

    #[test]
    fn bck_hidden_selected_search_row_keeps_playback() {
        let fixture = fixture(
            "hidden-row",
            &[
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
                ("bcks/run.bck", Some(ramp_clip(20, 2))),
            ],
        );
        let mut player = fixture.player();
        player.apply(Command::Select(fixture.identity(0)));
        player.advance(0.1);
        player.prepare_frame();
        let evaluations = player.evaluation_count();

        let visible: Vec<_> = player
            .search("RUN")
            .map(|row| row.identity.clone())
            .collect();
        assert_eq!(visible, [fixture.identity(1)]);
        assert!(!visible.contains(&fixture.identity(0)));
        assert_eq!(player.selected_identity(), Some(&fixture.identity(0)));
        assert_eq!(player.selected_label(), Some("LkAnm/bcks/walk.bck"));
        assert_eq!(player.playback_state(), PlaybackState::Playing);
        assert_frame(&player, 3.0);
        assert_eq!(player.evaluation_count(), evaluations);
        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::NoOp,
            "reselecting the active clip is a no-op even when filtered out"
        );
        assert_frame(&player, 3.0);
    }

    #[test]
    fn bck_identical_member_labels_disambiguated() {
        let fixture = fixture(
            "same-label",
            &[
                ("bcks/wait.bck", Some(ramp_clip(20, 2))),
                ("bcks/wait.bck", Some(ramp_clip(10, 2))),
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
            ],
        );
        let mut player = fixture.player();
        let labels: Vec<_> = player
            .catalog_rows()
            .iter()
            .map(|row| row.label.clone())
            .collect();
        assert_eq!(labels[2], "LkAnm/bcks/walk.bck");
        assert_ne!(labels[0], labels[1]);
        assert!(labels[0].starts_with("LkAnm/bcks/wait.bck [entry=0, resource=0"));
        assert!(labels[1].starts_with("LkAnm/bcks/wait.bck [entry=1, resource=1"));
        assert_eq!(player.search("wait").count(), 2);

        player.apply(Command::Select(fixture.identity(1)));
        assert_eq!(player.selected_identity(), Some(&fixture.identity(1)));
        assert_eq!(player.selected_label(), Some(labels[1].as_str()));
        assert_eq!(player.duration_frames(), Some(10));
        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );
        assert_eq!(player.duration_frames(), Some(20));
    }

    #[test]
    fn bck_bounded_steady_state_evaluations() {
        let fixture = fixture("evaluations", &[("bcks/walk.bck", Some(ramp_clip(20, 2)))]);
        let mut player = fixture.player();
        for _ in 0..5 {
            player.prepare_frame();
        }
        assert_eq!(
            player.evaluation_count(),
            0,
            "bind pose is never re-evaluated"
        );

        player.apply(Command::Select(fixture.identity(0)));
        assert_eq!(
            player.evaluation_count(),
            1,
            "selection evaluates frame 0 once"
        );
        player.prepare_frame();
        player.prepare_frame();
        assert_eq!(player.evaluation_count(), 1, "the selection pose is reused");

        // One evaluation per changed frame, no matter how many draws.
        for step in 1..=10 {
            player.advance(0.01);
            player.prepare_frame();
            player.prepare_frame();
            assert_eq!(player.evaluation_count(), 1 + step);
        }
        // Zero elapsed time and paused time evaluate nothing.
        player.advance(0.0);
        player.prepare_frame();
        player.apply(Command::Pause);
        for _ in 0..5 {
            player.advance(0.1);
            player.prepare_frame();
        }
        assert_eq!(player.evaluation_count(), 11);
        // A scrub to the already-published frame is free; a new frame costs one.
        player.apply(Command::Scrub(player.frame()));
        player.prepare_frame();
        assert_eq!(player.evaluation_count(), 11);
        player.apply(Command::Scrub(15.0));
        player.prepare_frame();
        player.prepare_frame();
        assert_eq!(player.evaluation_count(), 12);
        // Speed and loop changes alone do not evaluate.
        player.apply(Command::SetSpeed(0.5));
        player.apply(Command::SetLoop(LoopPreference::Once));
        player.prepare_frame();
        assert_eq!(player.evaluation_count(), 12);
    }

    #[test]
    fn bck_selection_prepares_once_frames_only_evaluate() {
        let fixture = fixture(
            "prepare-once",
            &[
                ("bcks/walk.bck", Some(ramp_clip(20, 2))),
                ("bcks/run.bck", Some(ramp_clip(10, 2))),
            ],
        );
        let mut player = fixture.player();
        assert_eq!(player.preparation_count(), 0);
        assert_eq!(player.evaluation_count(), 0);

        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::Applied
        );
        assert_eq!(player.preparation_count(), 1, "selection prepares once");
        assert_eq!(player.evaluation_count(), 1, "selection evaluates frame 0");

        // N changed frames: exactly N evaluations, zero further preparations.
        const CHANGED_FRAMES: u64 = 25;
        for step in 1..=CHANGED_FRAMES {
            player.advance(0.01);
            player.prepare_frame();
            player.prepare_frame();
            assert_eq!(player.evaluation_count(), 1 + step, "step {step}");
            assert_eq!(player.preparation_count(), 1, "step {step}");
        }
        player.apply(Command::Scrub(7.5));
        player.prepare_frame();
        assert_eq!(player.evaluation_count(), 2 + CHANGED_FRAMES);
        assert_eq!(player.preparation_count(), 1, "scrub does not prepare");

        // Re-selecting the active clip is a NoOp: no read, no preparation.
        assert_eq!(
            player.apply(Command::Select(fixture.identity(0))),
            CommandOutcome::NoOp
        );
        assert_eq!(player.preparation_count(), 1);

        // A different clip is prepared once more, then again only evaluated.
        assert_eq!(
            player.apply(Command::Select(fixture.identity(1))),
            CommandOutcome::Applied
        );
        assert_eq!(player.preparation_count(), 2);
        assert_eq!(player.evaluation_count(), 3 + CHANGED_FRAMES);
        player.advance(0.01);
        player.prepare_frame();
        assert_eq!(player.evaluation_count(), 4 + CHANGED_FRAMES);
        assert_eq!(player.preparation_count(), 2);
    }
}
