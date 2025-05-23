/// TODO:
/// Remove cloning the projection or just change it directly.
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    math::{
        Rect,
        bounding::{Aabb2d, BoundingVolume},
        vec2,
    },
    prelude::*,
    render::camera::CameraProjection,
    window::PrimaryWindow,
};
use std::ops::RangeInclusive;

/// Plugin that adds the necessary systems for [`AiO2DCam`] components to work.
#[derive(Default)]
pub struct AiO2DCamPlugin;

/// System set to allow ordering of [`AiO2DCamPlugin`].
#[derive(Debug, Clone, Copy, SystemSet, PartialEq, Eq, Hash)]
pub struct ProposeSystemSet;

/// System set to allow crate feature based configuration
#[derive(Debug, Clone, Copy, SystemSet, PartialEq, Eq, Hash)]
struct KeyboardSystemSet;

/// System set to allow crate feature based configuration
#[derive(Debug, Clone, Copy, SystemSet, PartialEq, Eq, Hash)]
struct MouseSystemSet;

/// System set to allow ordering of [`AiO2DCamPlugin`]
#[derive(Debug, Clone, Copy, SystemSet, PartialEq, Eq, Hash)]
struct ApplyProposedSystemSet;

/// Which keys move the camera in particular directions for keyboard movement
#[derive(Debug, Clone, PartialEq, Eq, Hash, Reflect)]
pub struct DirectionKeys {
    ///  The keys that move the camera up
    pub up: Vec<KeyCode>,
    ///  The keys that move the camera down
    pub down: Vec<KeyCode>,
    ///  The keys that move the camera left
    pub left: Vec<KeyCode>,
    ///  The keys that move the camera right
    pub right: Vec<KeyCode>,
}

/// A component that adds camera controls to an orthographic camera
#[derive(Component, Reflect)]
#[reflect(Component)]
#[require(Camera2d)]
pub struct AiO2DCam {
    /// The mouse buttons that will be used to drag and pan the camera
    pub grab_buttons: Vec<MouseButton>,
    /// The keyboard keys that will be used to move the camera
    pub move_keys: DirectionKeys,
    /// Speed for keyboard movement
    ///
    /// This is multiplied with the projection scale of the camera so the
    /// speed stays proportional to the current "zoom" level
    pub speed: f32,
    /// Whether camera currently responds to user input
    pub enabled: bool,
    /// When true, zooming the camera will center on the mouse cursor
    ///
    /// When false, the camera will stay in place, zooming towards the
    /// middle of the screen
    pub zoom_to_cursor: bool,
    /// The minimum scale for the camera
    ///
    /// The orthographic projection's scale will be clamped at this value when
    /// zooming in. Pass `f32::NEG_INFINITY` to disable clamping.
    pub min_scale: f32,
    /// The maximum scale for the camera
    ///
    /// The orthographic projection's scale will be clamped at this value when
    /// zooming out. Pass `f32::INFINITY` to disable clamping.
    pub max_scale: f32,
    /// The minimum x position of the camera window
    ///
    /// The orthographic projection will be clamped to this boundary both when
    /// dragging the window, and zooming out. Pass `f32::NEG_INFINITY` to disable
    /// clamping.
    pub min_x: f32,
    /// The maximum x position of the camera window
    ///
    /// The orthographic projection will be clamped to this boundary both when
    /// dragging the window, and zooming out. Pass `f32::INFINITY` to disable
    /// clamping.
    pub max_x: f32,
    /// The minimum y position of the camera window
    ///
    /// The orthographic projection will be clamped to this boundary both when
    /// dragging the window, and zooming out. Pass `f32::NEG_INFINITY` to disable
    /// clamping.
    pub min_y: f32,
    /// The maximum y position of the camera window
    ///
    /// The orthographic projection will be clamped to this boundary both when
    /// dragging the window, and zooming out. Pass `f32::INFINITY` to disable
    /// clamping.
    pub max_y: f32,
    /// The Proposed position of [`AiO2DCam`]. This should not be directly set by the user.
    proposed_cam_pos: Option<Vec2>,
    /// The Proposed scale of [`AiO2DCam`]. This should not be directly set by the user.
    proposed_cam_scale: Option<f32>,
}

#[derive(Resource, PartialEq, Eq, Default)]
#[cfg(feature = "bevy_egui")]
struct EguiWantsFocus {
    mouse: bool,
    keyboard: bool,
}

// todo: make run condition when Bevy supports mutable resources in them
#[cfg(feature = "bevy_egui")]
fn check_egui_wants_focus(
    mut contexts: Query<&mut bevy_egui::EguiContext>,
    mut wants_focus: ResMut<EguiWantsFocus>,
) {
    let ctx = contexts.iter_mut().next();
    let new_wants_focus = if let Some(ctx) = ctx {
        let ctx = ctx.into_inner().get_mut();
        (ctx.wants_pointer_input(), ctx.wants_keyboard_input())
    } else {
        (false, false)
    };
    wants_focus.set_if_neq(EguiWantsFocus {
        mouse: new_wants_focus.0,
        keyboard: new_wants_focus.1,
    });
}

fn do_camera_zoom(
    mut query: Query<(&mut AiO2DCam, &Camera, &Projection, &Transform)>,
    scroll_events: EventReader<MouseWheel>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    const ZOOM_SENSITIVITY: f32 = 0.001;

    let scroll_offset = scroll_offset_from_events(scroll_events);
    if scroll_offset == 0. {
        return;
    }

    let Ok(window) = primary_window.single() else {
        return;
    };

    for (mut aio_cam, camera, proj, transform) in &mut query {
        if !aio_cam.enabled {
            continue;
        }

        let view_size = camera.logical_viewport_size().unwrap_or(window.size());

        let mut proj = match &proj {
            Projection::Orthographic(proj) => proj.clone(),
            _ => continue,
        };

        let old_scale = proj.scale;

        match &mut aio_cam.proposed_cam_scale {
            Some(scale) => *scale *= 1. - scroll_offset * ZOOM_SENSITIVITY,
            opt @ None => *opt = Some(proj.scale * (1. - scroll_offset * ZOOM_SENSITIVITY)),
        }

        if !aio_cam.zoom_to_cursor {
            continue;
        }

        let cursor_normalized_viewport_pos = window
            .cursor_position()
            .map(|cursor_pos| {
                let view_pos = camera
                    .logical_viewport_rect()
                    .map(|v| v.min)
                    .unwrap_or(Vec2::ZERO);

                ((cursor_pos - view_pos) / view_size) * 2. - Vec2::ONE
            })
            .map(|p| vec2(p.x, -p.y));

        // Move the camera position to normalize the projection window
        let Some(cursor_normalized_view_pos) = cursor_normalized_viewport_pos else {
            continue;
        };

        proj.scale = aio_cam.proposed_cam_scale.unwrap();
        let proj_size = proj.area.max / old_scale;

        let cursor_world_pos =
            transform.translation.truncate() + cursor_normalized_view_pos * proj_size * old_scale;

        let proposed_cam_pos =
            cursor_world_pos - cursor_normalized_view_pos * proj_size * proj.scale;

        // As we zoom out, we don't want the viewport to move beyond the provided
        // boundary. If the most recent change to the camera zoom would move cause
        // parts of the window beyond the boundary to be shown, we need to change the
        // camera position to keep the viewport within bounds.
        match &mut aio_cam.proposed_cam_pos {
            Some(pos) => *pos -= proposed_cam_pos,
            opt @ None => *opt = Some(proposed_cam_pos),
        }
    }
}

/// Consumes `MouseWheel` event reader and calculates a single scalar,
/// representing positive or negative scroll offset.
fn scroll_offset_from_events(mut scroll_events: EventReader<MouseWheel>) -> f32 {
    let pixels_per_line = 100.; // Maybe make configurable?
    scroll_events
        .read()
        .map(|ev| match ev.unit {
            MouseScrollUnit::Pixel => ev.y,
            MouseScrollUnit::Line => ev.y * pixels_per_line,
        })
        .sum::<f32>()
}

/// `max_scale_within_bounds` is used to find the maximum safe zoom out/projection
/// scale when we have been provided with minimum and maximum x boundaries for
/// the camera.
fn max_scale_within_bounds(
    bounded_area_size: Vec2,
    proj: &OrthographicProjection,
    window_size: Vec2, //viewport?
) -> Vec2 {
    let mut proj = proj.clone();
    proj.scale = 1.;
    proj.update(window_size.x, window_size.y);
    let base_world_size = proj.area.size();
    bounded_area_size / base_world_size
}

/// Makes sure that the camera projection scale stays in the provided bounds
/// and range.
fn constrain_proj_scale(
    proj: &mut OrthographicProjection,
    bounded_area_size: Vec2,
    scale_range: &RangeInclusive<f32>,
    window_size: Vec2,
) {
    proj.scale = proj.scale.clamp(*scale_range.start(), *scale_range.end());

    // If there is both a min and max boundary, that limits how far we can zoom.
    // Make sure we don't exceed that
    if bounded_area_size.x.is_finite() || bounded_area_size.y.is_finite() {
        let max_safe_scale = max_scale_within_bounds(bounded_area_size, proj, window_size);
        proj.scale = proj.scale.min(max_safe_scale.x).min(max_safe_scale.y);
    }
}

/// Clamps a camera position to a safe zone. "Safe" means that each screen
/// corner is constrained to the corresponding bound corner.
///
/// Since bevy doesn't provide a `shrink` method on a `Rect` yet, we have to
/// operate on `Aabb2d` type.
fn clamp_to_safe_zone(pos: Vec2, aabb: Aabb2d, bounded_area_size: Vec2) -> Vec2 {
    let aabb = aabb.shrink(bounded_area_size / 2.);
    pos.clamp(aabb.min, aabb.max)
}

fn do_keyboard_movement(
    keyboard_buttons: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut camera_q: Query<(&mut AiO2DCam, &Transform, &Projection)>,
) {
    for (mut aio_cam, transform, proj) in &mut camera_q {
        let direction = aio_cam.move_keys.direction(&keyboard_buttons);

        let proj = match proj {
            Projection::Orthographic(proj) => proj,
            _ => continue,
        };

        let delta = time.delta_secs() * direction.normalize_or_zero() * aio_cam.speed * proj.scale;

        if delta == Vec2::ZERO {
            continue;
        }

        // The proposed new camera position
        match &mut aio_cam.proposed_cam_pos {
            Some(pos) => *pos += delta,
            opt @ None => *opt = Some(transform.translation.truncate() + delta),
        }
    }
}

fn do_mouse_movement(
    primary_window_q: Query<&Window, With<PrimaryWindow>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut camera_q: Query<(&mut AiO2DCam, &Camera, &Transform, &Projection)>,
    mut last_pos: Local<Option<Vec2>>,
) {
    let Ok(window) = primary_window_q.single() else {
        return;
    };
    let window_size = window.size();

    // Use position instead of MouseMotion, otherwise we don't get acceleration
    // movement
    let current_pos = match window.cursor_position() {
        Some(c) => vec2(c.x, -c.y),
        None => return,
    };
    let delta_device_pixels = current_pos - last_pos.unwrap_or(current_pos);

    for (mut aio_cam, camera, transform, projection) in &mut camera_q {
        if !aio_cam.enabled {
            continue;
        }

        let projection = match projection {
            Projection::Orthographic(proj) => proj,
            _ => continue,
        };

        let proj_area_size = projection.area.size();

        let delta = if !aio_cam
            .grab_buttons
            .iter()
            .any(|btn| mouse_buttons.pressed(*btn) && !mouse_buttons.just_pressed(*btn))
        {
            Vec2::ZERO
        } else {
            let viewport_size = camera.logical_viewport_size().unwrap_or(window_size);
            delta_device_pixels * proj_area_size / viewport_size
        };

        if delta == Vec2::ZERO {
            continue;
        }

        // The proposed new camera position
        match &mut aio_cam.proposed_cam_pos {
            Some(pos) => *pos -= delta,
            opt @ None => *opt = Some(transform.translation.truncate() - delta),
        }
    }
    *last_pos = Some(current_pos);
}

fn apply_proposed_zoom(
    camera_q: Query<(&mut AiO2DCam, &Camera, &mut Projection)>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = primary_window.single() else {
        return;
    };

    for (mut aio_cam, camera, mut proj) in camera_q {
        let proj = match &mut *proj {
            Projection::Orthographic(proj) => proj,
            _ => continue,
        };

        let Some(proposed_cam_scale) = aio_cam.proposed_cam_scale else {
            continue;
        };

        proj.scale = proposed_cam_scale;

        constrain_proj_scale(
            proj,
            aio_cam.rect().size(),
            &aio_cam.scale_range(),
            camera.logical_viewport_size().unwrap_or(window.size()),
        );

        aio_cam.proposed_cam_scale = None
    }
}

fn apply_proposed_pos(camera_q: Query<(&mut AiO2DCam, &Projection, &mut Transform)>) {
    for (mut aio_cam, proj, mut transform) in camera_q {
        let (Some(proposed_pos), Projection::Orthographic(proj)) = (aio_cam.proposed_cam_pos, proj)
        else {
            continue;
        };

        transform.translation = clamp_to_safe_zone(proposed_pos, aio_cam.aabb(), proj.area.size())
            .extend(transform.translation.z);

        aio_cam.proposed_cam_pos = None
    }
}

impl AiO2DCam {
    /// Returns (min, max) bound tuple
    fn bounds(&self) -> (Vec2, Vec2) {
        let min = vec2(self.min_x, self.min_y);
        let max = vec2(self.max_x, self.max_y);
        (min, max)
    }

    /// Returns the bounding `Rect`
    fn rect(&self) -> Rect {
        let (min, max) = self.bounds();
        Rect { min, max }
    }

    /// Returns the bounding `Aabb2d`
    fn aabb(&self) -> Aabb2d {
        let (min, max) = self.bounds();
        Aabb2d { min, max }
    }

    /// Returns the scale inclusive range
    fn scale_range(&self) -> RangeInclusive<f32> {
        self.min_scale..=self.max_scale
    }
}

impl Default for AiO2DCam {
    fn default() -> Self {
        Self {
            move_keys: DirectionKeys::arrows_and_wasd(),
            speed: 200.,
            grab_buttons: vec![MouseButton::Left, MouseButton::Right, MouseButton::Middle],
            enabled: true,
            zoom_to_cursor: true,
            min_scale: 0.00001,
            max_scale: f32::INFINITY,
            min_x: f32::NEG_INFINITY,
            max_x: f32::INFINITY,
            min_y: f32::NEG_INFINITY,
            max_y: f32::INFINITY,
            proposed_cam_pos: None,
            proposed_cam_scale: None,
        }
    }
}

impl DirectionKeys {
    /// No keys move the camera
    pub const NONE: Self = Self {
        up: vec![],
        down: vec![],
        left: vec![],
        right: vec![],
    };

    /// The camera is moved by the arrow keys
    pub fn arrows() -> Self {
        Self {
            up: vec![KeyCode::ArrowUp],
            down: vec![KeyCode::ArrowDown],
            left: vec![KeyCode::ArrowLeft],
            right: vec![KeyCode::ArrowRight],
        }
    }

    /// The camera is moved by the WASD keys
    pub fn wasd() -> Self {
        Self {
            up: vec![KeyCode::KeyW],
            down: vec![KeyCode::KeyS],
            left: vec![KeyCode::KeyA],
            right: vec![KeyCode::KeyD],
        }
    }

    /// The camera is moved by the arrow and WASD keys
    pub fn arrows_and_wasd() -> Self {
        Self {
            up: vec![KeyCode::ArrowUp, KeyCode::KeyW],
            down: vec![KeyCode::ArrowDown, KeyCode::KeyS],
            left: vec![KeyCode::ArrowLeft, KeyCode::KeyA],
            right: vec![KeyCode::ArrowRight, KeyCode::KeyD],
        }
    }

    fn direction(&self, keyboard_buttons: &Res<ButtonInput<KeyCode>>) -> Vec2 {
        let mut direction = Vec2::ZERO;

        if self.left.iter().any(|key| keyboard_buttons.pressed(*key)) {
            direction.x -= 1.;
        }

        if self.right.iter().any(|key| keyboard_buttons.pressed(*key)) {
            direction.x += 1.;
        }

        if self.up.iter().any(|key| keyboard_buttons.pressed(*key)) {
            direction.y += 1.;
        }

        if self.down.iter().any(|key| keyboard_buttons.pressed(*key)) {
            direction.y -= 1.;
        }

        direction
    }
}

impl Plugin for AiO2DCamPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                do_mouse_movement.in_set(MouseSystemSet),
                do_keyboard_movement.in_set(KeyboardSystemSet),
                do_camera_zoom.in_set(MouseSystemSet),
            )
                .in_set(ProposeSystemSet),
        )
        .add_systems(
            Update,
            (apply_proposed_zoom, apply_proposed_pos).in_set(ApplyProposedSystemSet),
        )
        .configure_sets(Update, ApplyProposedSystemSet.after(ProposeSystemSet))
        .register_type::<AiO2DCam>()
        .register_type::<DirectionKeys>();

        #[cfg(feature = "bevy_egui")]
        {
            app.init_resource::<EguiWantsFocus>()
                .add_systems(PostUpdate, check_egui_wants_focus)
                .configure_sets(
                    Update,
                    (
                        MouseSystemSet.run_if(|focus: Res<EguiWantsFocus>| !focus.mouse),
                        KeyboardSystemSet.run_if(|focus: Res<EguiWantsFocus>| !focus.keyboard),
                    ),
                );
        }
    }
}
