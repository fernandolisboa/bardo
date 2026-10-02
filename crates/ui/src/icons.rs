//! The icons Bardo draws: gpui-kit's default set plus the few Lucide icons
//! the navigation and the project stages need, embedded like the rest.

use std::borrow::Cow;

use bardo_app::{Destination, Stage};
pub use gpui_kit::assets::IconName as Lucide;
use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::{AssetSource, Result, SharedString};

icon_assets!(
    Extra,
    [
        CircleDollarSign,
        Clapperboard,
        Compass,
        Film,
        Image,
        LayoutTemplate,
        Lightbulb,
        Link,
        List,
        Lock,
        Mic,
        MonitorPlay,
        Pencil,
        Scissors,
        Send,
        Trash,
        Tv,
    ]
);

/// The default icons and Bardo's extra ones.
pub struct BardoAssets;

impl AssetSource for BardoAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Extra.load(path)? {
            Some(icon) => Ok(Some(icon)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(Extra.list(path)?);
        Ok(paths)
    }
}

pub fn destination(place: Destination) -> Lucide {
    match place {
        Destination::Research => Lucide::Compass,
        Destination::Themes => Lucide::Lightbulb,
        Destination::Projects => Lucide::Clapperboard,
        Destination::Personas => Lucide::User,
        Destination::Templates => Lucide::LayoutTemplate,
        Destination::Channels => Lucide::Tv,
        Destination::Accounts => Lucide::Link,
        Destination::Jobs => Lucide::List,
        Destination::Costs => Lucide::CircleDollarSign,
        Destination::Settings => Lucide::Settings,
    }
}

pub fn stage(stage: Stage) -> Lucide {
    match stage {
        Stage::Script => Lucide::FileText,
        Stage::Narration => Lucide::Mic,
        Stage::Scenes => Lucide::Image,
        Stage::Clips => Lucide::Film,
        Stage::Edit => Lucide::Scissors,
        Stage::Render => Lucide::MonitorPlay,
        Stage::Publish => Lucide::Send,
    }
}
