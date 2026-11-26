//! The product in use (#279, ADR-0042): a scene of an ad that shows the
//! product being used is drawn with the product's photo as a reference,
//! and the user checks the product in its picture, and in its clip,
//! against the real one before it is animated, played or rendered.
//! Bardo does not score likeness: the check is the user's, by eye.

use std::time::SystemTime;

use bardo_domain::VideoProjectId;

use crate::Bardo;
use crate::scenes::SceneError;

impl Bardo {
    /// Checks the product in scene `index`'s picture: it matches the real
    /// one. The scene may then be animated and rendered.
    pub fn check_product_picture(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), SceneError> {
        // An image or clip job saving the scene meanwhile would be written
        // over: a clip job also saves a scene it fails, or a clip it was
        // already making before the check was cleared.
        self.scenes_idle(project)?;
        if self.clips_running(project) {
            return Err(SceneError::Busy);
        }
        let mut plan = self.own_plan(project)?;
        if plan.scene_mut(index, SystemTime::now())?.check_product()? {
            self.scene_plans.save_scene(&plan, index)?;
        }
        Ok(())
    }

    /// Checks the product in scene `index`'s clip: it stays as it is all
    /// through. The cut then plays the clip instead of the still.
    pub fn check_product_clip(
        &self,
        project: VideoProjectId,
        index: usize,
    ) -> Result<(), SceneError> {
        // A clip job saving the scene meanwhile would be written over.
        self.scenes_idle(project)?;
        if self.clips_running(project) {
            return Err(SceneError::Busy);
        }
        let mut plan = self.own_plan(project)?;
        if plan
            .scene_mut(index, SystemTime::now())?
            .check_product_clip()?
        {
            self.scene_plans.save_scene(&plan, index)?;
        }
        Ok(())
    }
}
