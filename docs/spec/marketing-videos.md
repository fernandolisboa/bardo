# Marketing videos spec

- Status: Proposed. Defaults chosen on 2026-11-25; each slice may revisit its own before it starts.
- Date: 2026-11-25
- Parent issue: #272 (a sub-issue of the PRD, #4)
- Builds on: themes and ranking, templates, the scene plan with a cast (#246), the storyboard plan (#258), visual styles (#259), production modes and the whole-video estimate (#248), thumbnails (ADR-0021), uploads and exports (ADR-0008), costs and budgets
- Starts after: #258 and #259, which change the script, the scene plan and how images are drawn

## Goal

The user describes a product (a digital product, a physical product, a service, or only an idea), optionally with pictures of it, and gets an ad video that makes sense and persuades: Bardo researches the product and its audience, proposes angles, and the approved angle goes down the same road as any other video (script, narration, scenes, clips, cut, render, publish). The product on screen is the real product, what the ad says is what the user can stand behind, and each network gets the disclosure it asks for.

## Principles

- **A video project like any other.** An ad is a video project in a channel. It uses the channel's language, country, persona, visual style, production mode and network accounts, and every stage after the script works as it does today. What changes is where it starts (a product, not a niche), what the script and scene plan are asked for, and what the reviews check.
- **Only what the user confirmed.** The script may state about the product only **claims** the user confirmed. Only a statement the user typed starts confirmed. Everything a model wrote, from the brief, the product's page or the web, waits for the user. Bardo never invents a testimonial, a review, a rating, a number of customers, an award, a deadline or a result, and never presents a drawn person as a real customer, doctor or expert.
- **The product is not redrawn where it matters.** Shots of the product and the end card are composed on the computer from the user's real image, only scaled. Where an image model draws the product in use, the real image is its reference and the user checks the result against it before any clip or render uses it. A screenshot, and any composed picture, never goes to an image or video model.
- **Research is reading, not copying.** Research reads the product's page and what is public about its market to understand the product, its audience and how alternatives position themselves. The ad is original: no competitor's copy, slogan, footage or look, and no competitor named unless the user confirmed a comparison claim.
- **Free first, every paid step estimated.** Compositions are free. A research is paid per run, kept with the product and reused by every video of it, so no video pays for it. Every paid step shows its estimate and passes the provider's budget and the video's spend cap, as today.
- **Platform policies are product requirements.** Each network's commercial-content disclosure is set or reminded per upload, from who owns the product. The defaults follow each network's own rules; the user can change them in the upload review, and the choice is kept with the publication.
- **The user decides.** Nothing is published, rendered or exported for ads without the reviews below. Bardo flags; the user edits, confirms or acknowledges.

## Vocabulary

Each slice adds its terms to `docs/CONTEXT.md` when it lands.

| Term | Meaning |
| --- | --- |
| **Product** | Something to advertise, kept in a library owned by the user profile and reusable across channels and videos: its brief, its images, its sheet. |
| **Product brief** | What the user tells Bardo about a product: name, kind (digital product, physical product, service, idea), what it is, who it is for, the facts they stand behind (one per line), the offer (price, discount, deadline; optional), a link (optional), the call to action, whose it is (*mine* or *a client's*), things the ad must say and must never say. |
| **Product image** | A picture of the product the user imported: a *photo* (a physical product, packaging, a place) or a *screen* (a screenshot of an app or site), as the user marks it. Up to three per product, plus an optional logo. Importing asks the user to confirm they may use the images and the brand; the confirmation is kept with the product. A product may have none. |
| **Concept product** | A product of kind *idea*. Its videos present it as an idea, never as something that exists. With no image of its own, its picture is a *concept image* drawn from the description. |
| **Product sheet** | What Bardo knows about a product after research: a summary, the category, audience segments with what they want and what holds them back, the claims, objections and answers, alternatives and how the product differs, how the product looks (from its images), the sources read, and whether the category is one networks restrict. The user edits all of it. |
| **Claim** | One statement about the product an ad could make: a benefit, a feature, a proof (a number, an award, a rating), an offer or a comparison. It has a source (typed by the user, drawn from the brief's text, the product's page, a web page with its link) and a state: *confirmed*, *found* (waits for the user) or *rejected*. A claim is *sensitive* when it is a superlative, a health, money or results promise, a guarantee, a number, a deadline or a comparison that names a competitor. |
| **Ad framework** | The shape of an ad's argument, from a fixed catalog: problem and solution, demonstration, before and after, how it works, the story behind it, old way and new way, three reasons, myth and fact, offer. No framework is a testimonial. |
| **Ad angle** | One idea for an ad of a product in a channel: a working title, the angle, its framework, the audience segment it speaks to, a draft hook and the claims it leans on. Claude suggests angles, the decision engine ranks them, and the user edits, discards or approves each one. |
| **Ad project** | A video project started by approving an ad angle. It keeps its product and its own copy of what this video uses from it (the **Brief stage**). |
| **Brief stage** | The first stage of an ad project: the angle, the confirmed claims this video may use (each can be switched off), the offer, the call to action, and the link or handle to show. Confirming it opens the Script stage; changing it later marks the script, its claims check and the metadata for review. |
| **Claims check** | A reading of the ad's script and post texts, by the text generator, that ties every statement about the product to a claim of the Brief stage or flags it as *unbacked*. |
| **Scene kind** | In an ad project's scene plan: *drawn* (as today), *product in use* (drawn with the product's photo as reference), *product shot* (composed from the real image) or *end card* (composed). The user can change a scene's kind. |
| **Composed picture** | A product shot or an end card: made by Bardo on the computer, free, and never sent to an image or video model, neither as a scene to animate nor as a reference or a set for anything else. |
| **Product shot** | A composed scene picture: the real product image, whole and only scaled, over a **backdrop**. |
| **Backdrop** | What a composed picture sits on: *plain* (a color or gradient, free) or *drawn* (an empty surface or set drawn by the image generator in the video's style, with no product, text or logo). |
| **Product check** | The user's confirmation that a *product in use* picture or clip shows the product as it really is, made with the real image beside it. |
| **End card** | The ad's last picture, composed by Bardo: the call to action, the link or handle and, optionally, the offer, the product image and the logo, with text in Bardo's own fonts. |
| **Commercial disclosure** | What a network asks of a post that promotes something: *your brand* (the user's own product) or *paid partnership* (someone else's, promoted for them). Apart from the synthetic-content disclosure, which stays as it is. |
| **Placement export** | An ad project's cut rendered in the shapes ad managers ask for (4:5 and 1:1, beside 9:16 and 16:9), with the text fields to paste. Bardo never talks to an ad manager. |

## The flow

1. **Product.** On a new Products screen (Strategy, beside Research and Themes) the user creates a product: fills the brief, imports up to three images and an optional logo, and says whose it is. Nothing here is paid.
2. **Research (paid per run, optional).** *Research this product* shows its estimate, then Claude reads the brief, looks at the images, fetches the product's link and searches the web within fixed limits, and writes the product sheet with every claim tied to its source. *Write the sheet from my brief* does the same without the web, for a few cents; the user can also fill the sheet by hand for nothing.
3. **Sheet review.** The user reads the sheet, edits it, and confirms or rejects each found claim. Only confirmed claims can reach a script.
4. **Angles.** For a product and a channel, Claude suggests five angles and the decision engine ranks them. The user edits, discards or approves. Approving starts an ad project in that channel.
5. **Brief stage.** The project opens on its brief: the angle, the claims this video may use, the offer, the call to action, the link or handle, the length (30 seconds unless changed; 15 seconds and 1 minute are the other usual ones). The user confirms it.
6. **Script (paid).** Claude writes the ad's narration from the ad script template: the framework's steps, only the listed claims, the call to action as given. The claims check runs with it and flags unbacked statements; the user edits and can check again.
7. **Narration, music.** As today, with the channel's persona unless the project picks another. A persona can also perform the ad through the capture flow, unchanged, with the synthetic-content disclosure it already brings.
8. **Scenes.** The ad scene plan gives each scene a kind. Product shots and the end card are composed at once, for free. Drawn scenes follow the storyboard rules (#258) and the video's style (#259); *product in use* scenes are drawn with the real photo as reference and wait for the product check.
9. **Clips.** The production mode animates drawn scenes as today. Composed pictures stay stills with motion in every mode. A *product in use* scene is animated only after its product check.
10. **Cut, render.** As today. The render review lists *product in use* scenes whose check is missing and does not start until each is checked or turned into a product shot.
11. **Publish.** Post texts come from the ad metadata template and pass the claims check. The upload review adds the commercial disclosure per network, the sensitive claims used, any unbacked statements and, for a concept product, the concept notice. Exports carry the same reminders. *Export for ads* renders the placement shapes.

## Products and images

- A product belongs to the user profile, like a persona. Its images and logo are kept in the database with it, like a persona's look, so every kind of backup carries them. The settings file carries the briefs and sheets, not the images.
- Images are PNG, JPEG or WebP, 600 to 6,000 px a side, at most 10 MB each; the user's file is never changed. The user marks each as a *photo* or a *screen* (a photo unless changed). A photo with transparency is taken as already cut out.
- The logo is a PNG, used only on the end card.
- **Facts the user stands behind**, one per line in the brief, are the only claims that start confirmed.
- **A product with no image** is allowed for every kind (a service often has none). Its ads have no product shot and no *product in use* scene: drawn scenes and an end card of text (with the logo, if any). A product of kind *idea* can get a concept image instead (see Concept products).
- Whose it is: *mine* or *a client's*. It sets the default commercial disclosure and nothing else.
- Deleting a product is refused while a project uses it; archiving hides it from the list.

## Research and the product sheet

- **A new domain port, the web researcher**, beside the text generator: given instructions, a prompt, pictures and limits, it returns notes and the list of pages it read (link, title, when). Claude implements it with Anthropic's server-side web search and web fetch tools, so the app runs no browser and no scraper. The adapter is validated against the provider's current documentation and recorded fixtures.
- **Limits.** At most 8 searches and 5 page fetches per run, and at most 20,000 tokens read from any one page; the product's link is fetched first. The limits are part of the request, so a run cannot go past them and its estimate has a ceiling.
- **Two steps in one job.** The research call writes notes with sources. A second call, with no tools, turns the brief and the notes into the sheet as JSON in a fixed schema. The job checkpoints between them, so a resumed job does not pay for the research twice.
- **What the web says is data.** The research call has only the search and fetch tools and can change nothing in Bardo. Its result is a sheet the user reviews before anything uses it, and every claim it wrote names where it came from.
- **Sensitive categories.** The sheet says when the product falls in a category networks restrict (health and medicines, financial products, alcohol, gambling, weight loss, adult, political). The Products screen and the upload review then show a notice that each network has its own rules for the category. Bardo does not refuse the product.
- **Researching again** is a new paid run. It adds what is new and leaves confirmed and rejected claims as they are.
- **Costs.** A new cost purpose, *Product research*, recorded under no channel. Searches are a new meter in the rate table (US$ 10 per 1,000 searches on Anthropic's published price, checked 2026-11-25); fetched pages cost only their tokens. For scale: a full run is expected between US$ 0.20 and US$ 0.80, mostly the tokens of what it reads. The estimate shown is the ceiling the limits allow; the first real-key check measures a typical run.

## Claims

- **Who starts confirmed.** A fact the user typed in the brief, and a claim the user adds on the sheet. Every claim a model wrote starts *found*, whatever its source, including one it drew from the brief's own text: a paraphrase can change what was said.
- **Changing a claim.** The user can confirm or reject a found claim, reject a confirmed one, and bring a rejected one back. Editing a claim's text is the user's own statement: it stays in its state, with the time of the edit. Bardo keeps when each claim was confirmed and from which source.
- **Sensitive.** Proposed by the sheet call, always on for a claim that contains a digit, and switchable by the user either way. Sensitive claims can be confirmed and used; each one a video uses is listed again in its upload review and in its export's `metadata.txt`.
- **Language.** Claims stay as written. The ad templates write them in the channel's language.
- **A video's copy.** The Brief stage copies the claims it uses, so editing a product never changes a video silently. When a copied claim is later edited or rejected in the product, the Brief stage shows it and the user keeps the copy or drops it; the upload review lists claims used that the product no longer confirms.
- **Never, whatever the brief says:** an invented testimonial, review, rating, customer count, award or press mention; a deadline or scarcity the brief does not state; a drawn person presented as a real customer, doctor or expert; another brand's logo; a competitor's ad copy. The ad templates carry these rules in a fixed instruction after the template text, so editing a template cannot remove them.

## Angles

- **Suggestion.** One job per product and channel: Claude proposes five angles from the sheet (confirmed claims only), the channel (language, country, persona, niche) and the framework catalog; then the decision engine ranks them.
- **Ranking.** Three typed reasons, each a 0 to 100 score with a confidence: *audience fit* (does it speak to the segment it names), *hook* (would the first two seconds hold that viewer) and *clarity* (does a viewer know what the product is and what to do next). Priority weighs hook 40%, audience fit 35% and clarity 25%, in code like the theme ranking's weights. Bardo itself marks an angle that leans on a sensitive claim; that is not asked of the engine.
- **A theme with an angle record.** Approving an angle creates a theme whose title and angle are the ad angle's and whose niche is the product's category from the sheet, so the project it starts feeds the same template variables, stages, exports and metrics as any other. What a theme does not hold (the product, the framework, the segment, the hook, the claims and the ad ranking) lives in an ad angle record beside it. Ad angles show on the Products screen, not on Themes, and ad projects are left out of the past performance the theme ranking reads.
- Editing an angle drops its ranking until it is ranked again, as for themes.

## The ad script and the claims check

- **A new template kind, *Ad script*,** with the product, the audience segment, the angle, the framework and its steps, the numbered claims of the Brief stage, the offer, the call to action, the must-say and never-say notes, and the usual channel variables. It keeps the storyboard script's rules (#258): one continuous line, sentences that can be shown.
- **Short sentences.** A scene starts at a sentence and a sentence holds one scene at most, so an ad's cuts are its sentences: the template asks for sentences of about four to nine words, and for the call to action as a sentence of its own, last.
- **Lengths.** A new length, 15 seconds (about 30 to 38 words), joins 30 seconds, 1 minute and 3 minutes for every project. An ad project starts at 30 seconds, and its script aims at the length less 2 seconds, so the end card's tail fits inside it.
- **What it writes.** The hook in the first two seconds; then the framework's steps; the product named by the middle at the latest; the call to action last, in the user's words. Only listed claims; no superlative, number, deadline or comparison that is not one of them; no competitor named unless a confirmed comparison names it.
- **Concept products.** The script speaks of an idea: what it would do and why it matters, and a call to action that fits something that does not exist yet (follow, join the list, tell us what you think). No price and no "buy now" unless the brief's own call to action says so.
- **Claims check.** A call to the text generator, answered in a fixed schema. It runs with every generated ad script and, on request, after the user's edits (a small paid call with its estimate). It lists the script's statements about the product, each tied to a claim of the Brief stage or flagged *unbacked*; the Script stage marks the flagged sentences. A check belongs to the text and the Brief stage it read: a change to either shows it as out of date. Flags do not lock later stages. Starting a narration with unbacked statements says so beside the estimate, and the upload review and export make the user acknowledge them.
- **Text the user types** (an edited sentence aside, which the check reads again) is the user's own: the end card's headline and second line are not checked.

## Scenes of an ad

Built on the storyboard plan (#258) and visual styles (#259).

- **A new template kind, *Ad scene plan*.** One scene per sentence, so scenes run about 2 to 4 seconds. Each has a kind. The first scene shows the hook. When the product has an image, it is on screen by the time the narration names it and at least one scene is a product shot. The last scene is the end card, over the call to action. The plan may not ask for other brands, logos or real people; the product's own name and look are allowed.
- **Kinds the planner may use.** *Product shot* only when the product has an image; *product in use* only when it has a photo (or a concept image). The user can change a scene's kind in the scene inspector, within the same limits.
- **Product shots (free).** Composed by Bardo, in the frame's shape, from one product image:
  - a photo with transparency, or one Bardo can cut out (see the spike), stands on the backdrop with a soft shadow;
  - any other photo sits on the backdrop as a card with rounded corners;
  - a screen sits inside a plain device outline drawn by Bardo (a phone for a tall screenshot, a browser window for a wide one), with no maker's marks.
  The image is scaled to fit, never stretched, cut or redrawn, and placed with a margin, above the band captions use, so that neither the cut's motion nor a caption covers it. A warning shows when it is too small for the frame.
- **Backdrops.** Plain by default, in colors taken from the product image and adjustable. A drawn backdrop is one scene image's price: the image generator draws an empty surface or set in the video's style, and the drawing waits for review like any redrawn scene.
- **Composed pictures stay on the computer.** They are never animated, in any production mode, and the whole-video estimate counts them as free, before a plan exists too (an ad's typical plan has a scene about every 3 seconds, the composed ones free). They are never a reference for another scene: a composed scene has no place, is skipped as the previous shot, and the storyboard's look reference is the first drawn picture. They are never offered as the set of a performed take.
- **Product in use (paid, checked).** The scene's image request carries the product's photo as an object reference, the product's look from the sheet, and a fixed sentence: keep the product's shape, colors, label and proportions exactly as in the reference. References go in this order within the image model's limit: cast portraits, product, place, previous shot, look reference. The picture then waits for the **product check**: the scene inspector shows it beside the real image, and the user ticks it, draws it again, or picks *Use the real image instead*, which turns the scene into a product shot at no cost.
- **Screens are never drawn.** A screen is never a reference. For a product whose images are all screens, there is no *product in use* kind: drawn scenes show people and devices with the screen unreadable, and the interface appears only in product shots, from the real screenshot.
- **Clips.** A *product in use* scene is animated only after its product check, with a fixed sentence added to its motion prompt (the product stays rigid and unchanged; the camera or the person moves), and its clip waits for a product check of its own.
- **The check follows the kind.** Every *product in use* scene needs it; no other kind does. A drawn scene whose prompt happens to show the product has no reference and no check, which is why the planner is told to use the kind and the user can switch it.
- **Plans of other projects** are untouched: scenes without a kind are drawn scenes.

## Cutting a product out of its photo (spike)

A product shot looks best when the product stands free of its photo's background. The spike finds out whether Bardo can do that on the computer, free and offline, in the `vision` crate (ONNX Runtime, ADR-0024): a salient-object model under a license that allows commercial use (candidates: U²-Net, IS-Net, BiRefNet in its small sizes; RMBG is out for its license), measured on product photos (glass, reflections, white on white, fine edges) for quality, size and speed on the CPU and with DirectML. The result decides whether the cutout is built or the card layout stays the only one for opaque photos. No slice waits on it: the card layout always works.

## Concept products

- A product of kind *idea* is a concept, whatever pictures it has (a mock-up the user imported is still an idea). Changing its kind is how it stops being one.
- With no image of its own, *Draw a concept* has Claude write a prompt from the description (a new template kind, with a fixed instruction: an original product, no existing brand, logo, trade dress or text) and the image generator draw it. The drawing waits for review and can be drawn again; the kept one is the product's concept image, used where a photo would be (reference and product shots) and marked as drawn wherever it is kept.
- A concept's project shows a *Concept* badge. Its script follows the concept rules above. The upload review and the export show a notice the user acknowledges: the video shows an idea, not a product that exists.
- A project is a concept when its product is of kind *idea* at the time it starts; videos already made keep their badge.

## End card

- Composed by Bardo, like a thumbnail (ADR-0021): a backdrop, a headline (up to 40 characters), a second line (up to 60), an optional offer line, and, when the product has them, one product image and the logo. Text is drawn with Bardo's own fonts in one of the thumbnail looks.
- **Where the lines come from.** The headline starts as the Brief stage's call to action and the second line as its link or handle; the user can retype both on the card. The offer line is not typed: it shows the Brief stage's offer or is off.
- The user also picks the image, the backdrop, the look and where things sit, from places that keep clear of each network's own interface.
- **Timing.** It is the plan's last scene: on screen from the call to action's first word until 1.5 seconds after the narration's last. When that is less than 2.5 seconds, the tail grows until the card had 2.5. The music runs under the tail.
- **No captions over it.** Captions are not shown while the end card is on screen; the card says the call to action.
- Composed for each frame shape the project is rendered in.

## Publishing an ad

- **Metadata.** A new template kind, *Ad metadata*: the caption opens with the hook, uses only the Brief stage's claims, ends with the call to action, and includes the link where the network shows links in the text (YouTube's description, X); where it does not (Instagram, TikTok), it says where the link is, in the user's words from the brief. The claims check reads each network's text too.
- **Commercial disclosure.** The project's default comes from whose the product is: *mine* gives *your brand*, *a client's* gives *paid partnership*. The upload review shows it per network and the user can change it there; a change needs a new review, as for the other review fields. What each network takes (checked against each network's documentation on 2026-11-25; the slice checks again):

| Network | Your brand | Paid partnership |
| --- | --- | --- |
| YouTube | Nothing to set. | The API reads the paid-promotion box (`paidProductPlacementDetails.hasPaidProductPlacement`) but does not list it among the fields an upload can set. The review says to tick *Includes paid promotion* in YouTube Studio, and the publication links to the video's Studio page. If the documentation lists it as settable when the slice starts, Bardo sets it. |
| TikTok | The draft goes to the inbox, whose endpoint has no disclosure field. The review says to turn on *Disclose post content* and pick *Your brand* in TikTok before posting. | Same, with *Branded content*, which TikTok does not allow on a private post. |
| Instagram Reels | Nothing to set. | `is_paid_partnership` on the media container; optionally up to two brand accounts (`branded_content_sponsor_ids`) typed by the user. A refusal shows Meta's reason and nothing is resent without the label. |
| X | Nothing to set. | `paid_partnership` on the post. |
| Kick, and every export | A reminder line in `metadata.txt`. | A reminder line in `metadata.txt`. |

- **Synthetic-content disclosure.** Unchanged, with one more default for ad projects: on when a *product in use* scene is drawn in a photographic style (or in a style Bardo cannot tell, the channel's free notes), since that is a realistic picture of a real thing that was not filmed. Editable in the review, as today.
- **Review additions.** The upload review and the export of an ad project list the sensitive claims used, the unbacked statements left (and a check that is out of date), claims used that the product no longer confirms, the sensitive-category notice and the concept notice, each acknowledged before the upload or export starts. The acknowledgements are kept with the publication.

## Placement exports

- From the Publish stage of an ad project, *Export for ads* renders the cut in the chosen shapes: 4:5 (1080 × 1350), 1:1 (1080 × 1080), and the shape the cut is not in (16:9 for a vertical cut, 9:16 for a wide one). The cut's own shape is its ordinary render.
- These are render shapes, not editor shapes: the cut stays in its frame. For each shape, composed pictures (product shots, end card) are composed again for it, so the product and the text stay whole; drawn pictures and clips fill the frame from a window, centered unless the user moves it per clip in the export's review (in 16:9 from a vertical cut they show whole with bars, as the cut's framing already does); captions are laid out for the shape.
- The ad metadata job also writes the texts ad managers ask for (a primary text and a headline), kept with the project and passed by the claims check like the post texts.
- The export's review shows each shape before rendering, as a render review does, and asks for the same acknowledgements as an upload.
- Output: `Videos\Bardo\<project title> (<id>)\Ads\<shape>\` with the video (H.264, AAC, the loudness of the project's own render) and `ad.txt` holding the primary text, the headline, the call to action, the link and the review's reminders.
- No upload: the user takes the files to the ad manager.

## Costs

Paid: research (per run, never per video), the sheet, angles and their ranking, the script and its claims check, the narration, drawn scenes and drawn backdrops, concept images, clips as the production mode says, music, metadata, and a post where the network bills one (X bills a post with a link at its link price). Free: products, images, plain backdrops, product shots, the end card, product checks, placement exports.

For scale, a 30-second ad on the built-in rate table, about ten scenes of which two product shots and the end card are composed: about seven drawn images and a cast portrait at about US$ 0.10 each, a few cents of narration and music, and US$ 0.15 to 0.30 of text. That is about US$ 1.10 to 1.30 in Economy, and about US$ 0.60 more with the hook animated (three 5-second clips on Kling 2.6 at 720p). The whole-video estimate (#248) shows the real figure before anything is generated.

## Not in this feature

- Uploading to or reading from ad managers (Meta, TikTok, Google), campaign budgets, targeting, or ad performance.
- Testimonial ads, customer quotes on screen, and any person presented as a real customer or expert.
- Versions of an ad in another language (ADR-0023). An ad project does not offer them; a later slice can carry the product, the Brief stage and the end card across.
- Variants of one ad made automatically for testing. The user can approve several angles; each is its own project.
- Product pictures drawn in place of a real image the user has. The real image wins wherever the product is shown alone.
- Importing product data from store listings or feeds. The link is read once per research run.
- Removing or replacing a background with a paid model. The cutout is local or it is the card.
- Legal review of claims. Bardo keeps what the user confirmed and reminds; the decision is the user's.

## Acceptance

- From a brief and one photo, the user reaches a rendered 30-second ad whose product shots and end card show the photo undistorted, whose script uses only confirmed claims, and whose upload review shows the right disclosure for each network.
- From a description alone, the user reaches a concept ad that never says the product exists, with the concept notice acknowledged before upload.
- A claim a model wrote never reaches a script before the user confirms it; a rejected one never does.
- No composed picture and no image marked as a screen is ever sent to an image or video model, as a scene, a reference or a set.
- No *product in use* scene is animated or rendered before its product check.
- A research run never exceeds its search, fetch and page limits, no video pays for research, and every claim a model wrote names its source.
- An unbacked statement or a sensitive claim in the script or the post texts is acknowledged in the upload review and written in the export's reminders.
- Every paid call is estimated, recorded in Costs and passes the budget and the video's spend cap.
- Projects that are not ads behave exactly as before.

## Slices

In order (each is a sub-issue of #272). All start after #258 and #259 are merged; the spike can run at any time.

1. #273 Products: a library of things to advertise, with brief and images
2. #274 Product research on the web, and the product sheet with claims the user confirms (after 1)
3. #275 Ad angles: suggested, ranked, and approved into an ad project with its Brief stage (after 2)
4. #276 Ad script from confirmed claims, with a claims check and a 15-second length (after 3)
5. #277 Spike: cut a product out of its photo on the computer
6. #278 Ad scene plan with scene kinds, and product shots composed from the real image (after 4)
7. #285 Product shot layouts and backdrops: cutout, device outline, drawn backdrops (after 6)
8. #279 The product as a reference in drawn scenes, checked before clips and render (after 6)
9. #280 Concept products: a drawn concept image for an idea that does not exist yet (after 8)
10. #281 End card: call to action, link and offer composed over the product (after 6)
11. #282 Commercial disclosure per network, ad metadata and the ad's upload review (after 8)
12. #283 Placement exports: the cut in 4:5 and 1:1 for ad managers (after 10 and 11)
