---
id: world
title: World and characters
group: production
place: world
tour: world
---

# World and characters

The **World** holds the characters that come back across your videos: a grandmother who narrates the family stories, a detective, a mascot. Each has a fixed appearance, notes that always or never hold, and a **reference picture** per look, so every video that picks the character shows the same face. The project's characters live on [World](bardo:go/world); characters every project can pick live on the [global library's World tab](bardo:go/library/world). Writing and editing characters costs nothing; drawing a picture is paid, and shows its cost first. [Show me the screen](bardo:tour/world).

<a id="characters"></a>
## Your characters

The list holds the characters of the open project (or of the global library, on its tab). **New character** opens an empty form; picking a character opens it for editing. Archived characters are hidden until you tick **Show archived**. A character's name is unique among the characters of the same place, archived ones included, ignoring case: a project and the global library may each have a Rosa.

<a id="appearance"></a>
## Appearance and notes

- **Name** (required): at most 60 characters. The scene plan calls the character by this name.
- **Appearance** (required): face, hair, build and clothes, at most 1,500 characters. Every reference picture is drawn from this text, and the scene plan copies it as it is into the cast.
- **Always** and **Never**: what always holds (carries a red umbrella) and what never happens (never speaks on screen), at most 1,000 characters each. The scene planner reads them in every video that picks the character.

A new **appearance** drops the character's reference pictures in every look and the portraits of the videos that show it, since they no longer match. Before saving, Bardo says which looks and how many videos: **Save and drop them** confirms. Each picture is drawn again, and paid again, the next time it is needed. A new name or new notes keep the pictures.

<a id="pictures"></a>
## Reference pictures

A reference picture is the character's portrait in one **look**: a style of the catalog (photo, cartoon, manga and so on), or **a channel's notes**, kept by the channel and the text of its aesthetic notes. Each picture has its own record of how it was made (provider, model, prompt, when, and whether a video drew it, you drew it here or you saved it from a video's cast), and lives in the project's `World` folder, or in `Global\World` under the base folder for the global library.

- **Before the first scene that needs it**: a video that shows the character in a look with no picture draws one before that scene, as it does for any [person who comes back](scenes.md#cast), paid under that video. The picture is then kept in the world.
- **On demand**: pick a look, see the estimate and press **Draw picture**. The new picture waits for review: **Keep** makes it the one videos in that look use, replacing the earlier one, and **Discard** throws it away. It is paid under the character's project; a global character's under no project.

Every later video in the same look gets the kept picture for free: the estimate does not count it and no paid call is made. Other notes on a channel are another look, so they draw a new picture. Deleting the video that first drew a picture leaves the picture. **Remove** drops a kept picture; the next video in that look draws a new one.

<a id="archive"></a>
## Duplicate, archive and delete

- **Duplicate** copies the fields and the kept pictures under a new name.
- **Archive** hides the character from [Who appears](scenes.md#who-appears); videos that already picked it keep it. **Restore** brings it back.
- **Delete** removes the character with its pictures. It is refused while a video in progress uses it, and the message names those videos: archive the character, or archive or delete those videos first. Videos that showed it keep their portraits; their cast member becomes one of the video's own.

<a id="save"></a>
## Saving a video's person to the world

A person of a video's cast can become a character: on the [Scenes](scenes.md#cast) stage, **Save to World** keeps them in the project's world and **Save to global library** in the global library, with their portrait for the video's look. The video's person then is that character.

<a id="backup"></a>
## Backups and the settings file

[Backups](backup.md) carry the characters and their pictures. The [settings file](settings.md) does not: it carries preferences, not your work. Deleting a project deletes its characters.
