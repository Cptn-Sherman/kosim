use bevy::{
    app::{
        App, FixedPreUpdate, FixedUpdate, Plugin, PreStartup, PreUpdate, RunFixedMainLoop,
        RunFixedMainLoopSystems, Startup, Update,
    },
    ecs::{
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Res, ResMut},
    },
    prelude::{Deref, DerefMut},
};

pub mod binding;
pub mod cursor;
pub mod input;

#[derive(Resource)]
pub struct InputConfig {
    pub sensitivity: f32,
    pub gamepad_look_sensitivity: f32,
    pub mouse_look_sensitivity: f32,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            gamepad_look_sensitivity: 1.0,
            mouse_look_sensitivity: 0.1,
        }
    }
}

pub struct KosimInputPlugin;

impl Plugin for KosimInputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(InputConfig::default())
            .insert_resource(binding::Bindings::default())
            .init_resource::<DidFixedTimestepRunThisFrame>()
            .add_systems(PreStartup, cursor::initial_lock_cursor)
            .add_systems(Startup, cursor::initial_cursor_center)
            .add_systems(PreUpdate, clear_fixed_timestep_flag)
            .add_systems(FixedPreUpdate, set_fixed_time_step_flag)
            .add_systems(
                RunFixedMainLoop,
                (
                    (input::update_input_resource)
                        .chain()
                        .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                    (input::clear_input_resource.run_if(did_fixed_timestep_run_this_frame))
                        .chain()
                        .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
                ),
            )
            .add_systems(FixedUpdate, cursor::detect_toggle_cursor_system);
    }
}

/// A simple resource that tells us whether the fixed timestep ran this frame.
#[derive(Resource, Debug, Deref, DerefMut, Default)]
pub struct DidFixedTimestepRunThisFrame(bool);

/// Reset the flag at the start of every frame.
fn clear_fixed_timestep_flag(
    mut did_fixed_timestep_run_this_frame: ResMut<DidFixedTimestepRunThisFrame>,
) {
    did_fixed_timestep_run_this_frame.0 = false;
}

/// Set the flag during each fixed timestep.
fn set_fixed_time_step_flag(
    mut did_fixed_timestep_run_this_frame: ResMut<DidFixedTimestepRunThisFrame>,
) {
    did_fixed_timestep_run_this_frame.0 = true;
}

fn did_fixed_timestep_run_this_frame(
    did_fixed_timestep_run_this_frame: Res<DidFixedTimestepRunThisFrame>,
) -> bool {
    did_fixed_timestep_run_this_frame.0
}
