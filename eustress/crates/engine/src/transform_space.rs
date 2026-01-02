use bevy::prelude::*;

/// Transform space mode for tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformSpace {
    #[default]
    Local,  // Transformations relative to part orientation (default)
    World,  // Transformations relative to world axes
}

impl TransformSpace {
    pub fn toggle(&mut self) {
        *self = match self {
            TransformSpace::Local => TransformSpace::World,
            TransformSpace::World => TransformSpace::Local,
        };
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            TransformSpace::Local => "Local",
            TransformSpace::World => "World",
        }
    }
}

/// Resource tracking current transform space mode
#[derive(Resource, Default)]
pub struct TransformSpaceMode(pub TransformSpace);

/// Plugin for transform space management
pub struct TransformSpacePlugin;

impl Plugin for TransformSpacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TransformSpaceMode>();
    }
}
