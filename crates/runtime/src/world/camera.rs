use agge_core::{Room, clamp_view_origin, follow_view_origin};

#[derive(Default)]
pub(super) struct ViewCamera {
    pub(super) origin: [f32; 2],
    pub(super) room: [u32; 2],
    pub(super) follow: Option<String>,
    pub(super) requested: Option<[f32; 2]>,
    viewport: [u32; 2],
    dead: [f32; 2],
    smooth: f32,
}

impl ViewCamera {
    pub(super) fn new(room: &Room, viewport: [u32; 2]) -> Self {
        let size = room.dimensions(viewport);
        Self {
            origin: clamp_view_origin(room.camera.at, size, viewport),
            room: size,
            follow: room.camera.follow.clone(),
            requested: None,
            viewport,
            dead: room.camera.dead,
            smooth: room.camera.smooth,
        }
    }

    pub(super) fn update(&mut self, target: Option<[f32; 2]>, dt: f32, entering: bool) {
        if let Some(request) = self.requested.take() {
            self.origin = clamp_view_origin(request, self.room, self.viewport);
        } else if let Some(center) = target {
            let wanted =
                follow_view_origin(self.origin, center, self.room, self.viewport, self.dead);
            let follow = if entering || self.smooth == 0. {
                1.
            } else {
                (self.smooth * dt).min(1.)
            };
            for (origin, wanted) in self.origin.iter_mut().zip(wanted) {
                *origin = if follow == 1. {
                    wanted
                } else {
                    *origin + (wanted - *origin) * follow
                };
            }
        }
    }
}
