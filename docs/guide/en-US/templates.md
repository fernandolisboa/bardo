---
id: templates
title: Templates
group: production
place: templates
tour: templates
---

# Templates

Templates are the prompts Bardo sends to the AI. Change one to change how Bardo writes scripts, plans scenes, words music prompts or drafts posts, for every channel of a project, or for all of them. They live on [Templates](bardo:go/templates). [Show me the screen](bardo:tour/templates).

<a id="kinds"></a>
## Kinds of template

There is one template per kind of generation:

- **Script**: the video's script ([Script](script.md)). An ad's script comes from the Ad script template instead.
- **Image prompts**: the scene plan and each scene's image prompt ([Scenes and images](scenes.md#plan)).
- **Music prompt**: the prompt for your music tool ([Script](script.md#music)).
- **Metadata**: each network's title, description and tags at the Publish stage. An ad's come from the Ad metadata template instead.
- **Thumbnail**: a long video's YouTube thumbnail text and the prompt for its background ([Thumbnail](uploading.md#thumbnail)).
- **Chapters**: a long video's YouTube chapters, from the narration's sentences ([Chapters](uploading.md#chapters)).
- **Localized version**: a version's script, adapted sentence by sentence for another language or country ([Versions in other languages](script.md#versions)). Besides the target channel's facts, it reads `{{source_language}}`, `{{source_country}}`, the original title and its numbered sentences.
- **Look prompt**: the image prompt for a persona's drawn look, from your description ([The look](personas.md#look)). It reads `{{persona}}` and `{{look_description}}`, and it must keep declining real people.
- **Take prompt**: the prompt a take model gets for one take of a capture ([The take's prompt](capture.md#take-prompt)). It reads `{{look}}`, `{{set}}`, `{{words}}` and `{{instruction}}`: what the model does with the take's mode, which the prompt should keep.
- **Ad angles**: the ad angles proposed for a product on a channel ([Ad angles](products.md#angles)). Besides the channel's facts, it reads `{{product}}`, `{{product_audience}}`, `{{product_claims}}` (the confirmed claims, numbered), `{{product_notes}}`, `{{ad_frameworks}}`, `{{angles_to_avoid}}` and `{{angle_count}}`. After its instructions Bardo always adds the rules of every ad, which no edit removes: state only what the numbered claims state; invent no testimonial, review, rating, number of customers, award or press mention; state no deadline or scarcity the advertiser's notes do not state; never present a drawn person as a real customer, doctor or expert; never show another brand's logo or copy a competitor's ad.
- **Ad script**: the script of an ad ([Script](script.md#write)). Besides the channel's facts, the theme and `{{target_length}}` (2 seconds under the ad's length), it reads `{{product}}` (its name, kind and summary), `{{product_concept}}` (whether the product exists or is a concept), `{{ad_segment}}` (who the ad speaks to), `{{ad_hook}}` (the angle's first words), `{{ad_framework}}` (the ad's format and its steps), `{{brief_claims}}` (the claims of the Brief stage, numbered), `{{offer}}`, `{{call_to_action}}`, `{{must_say}}` and `{{never_say}}`. After its instructions Bardo always adds the rules of every ad, as for ad angles, and for a product of kind *idea* the rules of a concept: speak of it as an idea, never say or imply it can be bought, ordered, downloaded or used today, and state no price and no "buy now" unless your own call to action says so. An edit that leaves `{{brief_claims}}` or `{{never_say}}` out does not leave them unsent: Bardo adds the brief's claims and what must never be said after the prompt. The [claims check](script.md#claims) that reads the script afterwards is not a template.
- **Ad metadata**: each network's post of an ad at the Publish stage ([Before an ad goes out](uploading.md#ad-review)). Besides the channel's facts, the theme, `{{script}}` and `{{networks}}`, it reads `{{product}}`, `{{product_concept}}`, `{{ad_hook}}`, `{{brief_claims}}`, `{{offer}}`, `{{call_to_action}}` and `{{link}}` (where the ad sends viewers, from the Brief stage). After its instructions Bardo always adds the rules of every ad, and for a product of kind *idea* the rules of a concept; the brief's claims and what must never be said are always sent, even by an edit that leaves `{{brief_claims}}` out. The check of the post texts that follows is not a template.
- **Ad scene plan**: the scene plan of an ad ([The scenes of an ad](scenes.md#ad)), in place of the image prompts template. Besides what that one reads (the channel's facts, `{{visual_style}}`, the theme, `{{narration_sentences}}` and `{{frame_shape}}`), it reads `{{product}}`, `{{product_concept}}`, `{{call_to_action}}`, `{{product_images}}` (the product's images, numbered from 0, with what each one is and its shape) and `{{scene_kinds}}` (the kinds a scene of this ad may be, which follow the product's images). After its instructions Bardo always adds the rules of every ad and the rules of an ad's scenes: only the kinds listed, only the last scene as the end card, nothing of the product drawn or redrawn for a product shot or the end card, no other brand, logo or real person in a drawn picture, and no text, caption or watermark in a picture beyond the product's own name on the product. An edit that leaves `{{product_images}}` or `{{scene_kinds}}` out does not leave them unsent: Bardo adds both after the prompt.
- **Concept prompt**: the image prompt for the concept image of a product of kind *idea* ([A concept image for an idea](products.md#concept)). It reads `{{product}}` and `{{product_audience}}`. After its instructions, and again after its prompt, Bardo always adds an instruction no edit removes: draw an original product, with no existing brand, logo or trade dress, and no text at all. The image generator is told the same after Claude's prompt.

<a id="project"></a>
## This project's or global

Each kind has a global template, and a project may have its own. On a project's **Templates** entry, the screen says which one the project uses: **This project's own template**, or **Global template** when it has none of its own. Saving the global template changes it for every project that uses it, so Bardo lists those projects and asks first. **Make it this project's own** gives the project its own template of that kind, starting as a copy of the global one's latest version; from then on the project's generations use it, and the global one stays as it is for the other projects. Generations made before keep the version they used. The **Templates** tab of the [Global library](global-library.md) edits the global templates. A channel's videos use the templates of the channel's project.

<a id="versions"></a>
## Versions

Every save adds a version, numbered in order; the newest is **Current** and is what generations use. Older versions stay in the list: pick one to load its text into the editor, and save it to make it current again. Each generation records the version it used, so a script's **Details** tells which one wrote it.

When an update to Bardo improves a default you never changed, the new default comes in as the next version. A template you edited stays as you left it; **Load Bardo's default** shows what changed.

<a id="fields"></a>
## Instructions and prompt

A template has two parts, each up to 8,000 characters:

- **Instructions**: the standing rules, such as the role, the tone and the format of the answer.
- **Prompt**: the task itself, with the project's facts placed in it through variables. It cannot be empty.

<a id="variables"></a>
## Variables

Write a variable as `{{name}}`; each generation replaces it with the project's value. The list under the editor shows the variables this kind of template can use and what each holds, such as `{{channel_name}}`, `{{persona}}` or `{{narration_sentences}}`. `{{video_title}}` is the YouTube title when written, else the project's. `{{target_length}}` and `{{frame_shape}}` follow the video's [format](projects.md#format): how long the script runs (or the project's [length](projects.md#length) when it picks one) and the shape of the pictures. A script template without `{{target_length}}` ignores both. `{{visual_style}}` (image prompts and thumbnail templates) is the video's [visual style](channels.md#style): that one was chosen and the words Bardo adds to every image for it, or that none was chosen. Bardo adds a chosen style to the images whatever the template says. A template whose prompt part you edited and that lacks the variable never tells Claude a style was chosen, so Claude keeps writing the look of the notes into every prompt: add a line with `{{visual_style}}` to it. `{{fixed_cast}}` (image prompts templates) holds the characters picked under [Who appears](scenes.md#who-appears), each with its appearance, notes, belongings and this video's instruction; `{{fixed_places}}` and `{{fixed_objects}}` hold the picked places and objects the same way. A template that does not place one of them gets it after the prompt. A misspelled or unknown variable, or one missing its closing `}}`, keeps the template from being saved.

<a id="save"></a>
## Saving and starting over

**Save as new version** keeps your text as the next version; when nothing changed, no version is added. **Discard changes** goes back to the current version. **Load Bardo's default** puts the text Bardo shipped with into the editor, to save as a new version if you want it back.
