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
- **Only what the user confirmed.** The script may state about the product only **claims** the user confirmed. A claim comes from the user's own brief, from the product's page or from a cited web source; the last two wait for the user's confirmation. Bardo never invents a testimonial, a review, a rating, a number of customers, an award, a deadline or a result, and never presents a drawn person as a real customer, doctor or expert.
- **The product is not redrawn where it matters.** Shots of the product and the end card are composed on the computer from the user's real image, pixel for pixel. Where an image model draws the product in use, the real image is its reference and the user checks the result against it before any clip or render uses it. A screenshot never goes to an image or video model.
- **Research is reading, not copying.** Research reads the product's page and what is public about its market to understand the product, its audience and how alternatives position themselves. The ad is original: no competitor's copy, slogan, footage or look, and no competitor named unless the user confirmed a comparison claim.
- **Free first, every paid step estimated.** Compositions are free. Research is paid once per product and reused by every video of it. Every paid step shows its estimate and passes the provider's budget and the video's spend cap, as today.
- **Platform policies are product requirements.** Each network's commercial-content disclosure is set or reminded per upload, from who owns the product. The defaults follow each network's own rules; the user can change them in the upload review, and the choice is kept with the publication.
- **The user decides.** Nothing is published, rendered or exported for ads without the reviews below. Bardo flags; the user edits, confirms or acknowledges.

## Vocabulary

Each slice adds its terms to `docs/CONTEXT.md` when it lands.

| Term | Meaning |
| --- | --- |
| **Product** | Something to advertise, kept in a library owned by the user profile and reusable across channels and videos: its brief, its images, its sheet. |
| **Product brief** | What the user tells Bardo about a product: name, kind (digital product, physical product, service, idea), what it is, who it is for, the offer (price, discount, deadline; optional), a link (optional), the call to action, whose it is (*mine* or *a client's*), things the ad must say and must never say. |
| **Product image** | A picture of the product the user imported: a *photo* (a physical product, packaging, a place) or a *screen* (a screenshot of an app or site). Up to three per product, plus an optional logo. Importing asks the user to confirm they may use the images and the brand; the confirmation is kept with the product. |
| **Concept product** | A product of kind *idea* with no real image. Its picture is a *concept image* drawn from the description; its videos present it as an idea, never as something that exists. |
| **Product sheet** | What Bardo knows about a product after research: a summary, the category, audience segments with what they want and what holds them back, the claims, objections and answers, alternatives and how the product differs, how the product looks (from its images), the sources read, and whether the category is one networks restrict. The user edits all of it. |
| **Claim** | One statement about the product an ad could make: a benefit, a feature, a proof (a number, an award, a rating, a quote), an offer or a comparison. It has a source (the brief, the product's page, a web page with its link) and a state: *confirmed*, *found* (waits for the user) or *rejected*. Claims from the brief start confirmed. A claim is *sensitive* when it is a superlative, a health, money or results promise, a guarantee, a number, a deadline or a comparison that names a competitor. |
| **Ad framework** | The shape of an ad's argument, from a fixed catalog: problem and solution, demonstration, before and after, how it works, the story behind it, old way and new way, three reasons, myth and fact, offer. No framework is a testimonial. |
| **Ad angle** | One idea for an ad of a product in a channel: a working title, the angle, its framework, the audience segment it speaks to, a draft hook and the claims it leans on. Claude suggests angles, the decision engine ranks them, and the user edits, discards or approves each one. Stored as a theme whose subject is a product. |
| **Ad project** | A video project started by approving an ad angle. It keeps its product and its own copy of what this video uses from it (the **Brief stage**). |
| **Brief stage** | The first stage of an ad project: the angle, the confirmed claims this video may use (each can be switched off), the offer, the call to action, the link or handle shown, and the end card. Confirming it opens the Script stage; changing it later marks the script for review. |
| **Claims check** | A reading of the ad script that ties every statement about the product to a confirmed claim or flags it as *unbacked*. |
| **Scene kind** | In an ad project's scene plan: *drawn* (as today), *product in use* (drawn with the product image as reference), *product shot* (composed from the real image) or *end card* (composed). |
| **Product shot** | A scene picture composed on the computer: the real product image, whole and undistorted, over a **backdrop**. Free, never sent to a video model. |
| **Backdrop** | What a composed picture sits on: *plain* (a color or gradient, free) or *drawn* (an empty surface or set drawn by the image generator in the video's style, with no product, text or logo). |
| **Product check** | The user's confirmation that a drawn picture or clip shows the product as it really is, made with the real image beside it. |
| **End card** | The ad's last picture, composed by Bardo: the product image, the call to action, the link or handle and, optionally, the offer and the logo, with text in Bardo's own fonts. |
| **Commercial disclosure** | What a network asks of a post that promotes something: *your brand* (the user's own product) or *paid partnership* (someone else's, promoted for them). Apart from the synthetic-content disclosure, which stays as it is. |
| **Placement export** | An ad project's cut rendered in the shapes ad managers ask for (4:5 and 1:1, beside 9:16 and 16:9), with the text fields to paste. Bardo never talks to an ad manager. |

## The flow

1. **Product.** On a new Products screen (Strategy, beside Research and Themes) the user creates a product: fills the brief, imports up to three images and an optional logo, and says whose it is. Nothing here is paid.
2. **Research (paid once, optional).** *Research this product* shows its estimate, then Claude reads the brief, looks at the images, fetches the product's link and searches the web within fixed limits, and writes the product sheet with every found claim tied to its source. *Write the sheet from my brief* does the same without the web, for a few cents; the user can also fill the sheet by hand for nothing.
3. **Sheet review.** The user reads the sheet, edits it, and confirms or rejects each found claim. Only confirmed claims can reach a script.
4. **Angles.** For a product and a channel, Claude suggests five angles and the decision engine ranks them. The user edits, discards or approves. Approving starts an ad project in that channel.
5. **Brief stage.** The project opens on its brief: the angle, the claims this video may use, the offer, the call to action, the link or handle, the end card's lines, the length (30 seconds unless changed; 15 seconds and 1 minute are the other usual ones). The user confirms it.
6. **Script (paid).** Claude writes the ad's narration from the ad script template: the framework's steps, only the listed claims, the call to action as given. The claims check runs with it and flags unbacked statements; the user edits and can check again.
7. **Narration, music.** As today, with the channel's persona unless the project picks another. A persona can also perform the ad through the capture flow, unchanged.
8. **Scenes.** The ad scene plan gives each scene a kind. Product shots and the end card are composed at once, for free. Drawn scenes follow the storyboard rules (#258) and the video's style (#259); scenes that show the product in use are drawn with the real image as reference and wait for the product check.
9. **Clips.** The production mode animates drawn scenes as today. Product shots and the end card stay stills with motion in every mode. A scene that shows the product is animated only after its product check.
10. **Cut, render.** As today. The render review lists scenes whose product check is missing and does not start until each is checked or replaced by a product shot.
11. **Publish.** Metadata comes from the ad metadata template. The upload review adds the commercial disclosure per network, the list of sensitive claims the script uses, any unbacked statements and, for a concept product, the concept notice. Exports carry the same reminders. *Export for ads* renders the placement shapes.

## Products and images

- A product belongs to the user profile, like a persona. Backup and restore carry products with their images. The settings file carries the briefs and sheets, not the images.
- Images are PNG, JPEG or WebP, 600 to 6,000 px a side, at most 10 MB each, copied into Bardo's data; the user's file is never changed. Each is marked *photo* or *screen* by the user (Bardo proposes one from the picture's shape and content, the user can switch it). A photo with transparency is taken as already cut out.
- The logo is a PNG, used only on the end card.
- Whose it is: *mine* or *a client's*. It sets the default commercial disclosure and nothing else.
- Deleting a product is refused while a project uses it; archiving hides it from the list.

## Research and the product sheet

- **A new domain port, the web researcher**, beside the text generator: given instructions, a prompt, pictures and limits, it returns notes and the list of pages it read (link, title, when). Claude implements it with Anthropic's server-side web search and web fetch tools, so the app runs no browser and no scraper. The adapter is validated against the provider's current documentation and recorded fixtures.
- **Limits.** At most 8 searches and 5 page fetches per research; the product's link is fetched first. The limits are part of the request, so a research cannot run past them.
- **Two steps in one job.** The research call writes notes with sources. A second call, with no tools, turns the brief and the notes into the sheet as JSON in a fixed schema. The job checkpoints between them, so a resumed job does not pay for the research twice.
- **What the web says is data.** The research call has only the search and fetch tools and can change nothing in Bardo. Its result is a sheet the user reviews before anything uses it, and every found claim names the page it came from.
- **Sensitive categories.** The sheet says when the product falls in a category networks restrict (health and medicines, financial products, alcohol, gambling, weight loss, adult, political). The Products screen and the upload review then show a notice that each network has its own rules for the category. Bardo does not refuse the product.
- **Researching again** adds what is new and leaves confirmed and rejected claims as they are.
- **Costs.** A new cost purpose, *Product research*, recorded under no channel. Searches are a new meter in the rate table (US$ 10 per 1,000 searches on Anthropic's published price, checked 2026-11-25); fetched pages cost only their tokens. For scale: a full research is expected between US$ 0.20 and US$ 0.80, mostly the tokens of what it reads; the first real-key check measures it and the estimate follows.

## Claims

- A claim's text is in the channel's language when used; the sheet keeps it in the language it was found or typed in.
- **Confirming** a found claim is the user saying it is true and theirs to make. Bardo keeps who confirmed, when and from which source.
- **Sensitive claims** can be confirmed and used. Each one the script uses is listed again in the upload review and in the export's `metadata.txt`.
- **A quote** is a claim of kind proof with its author, typed by the user, who confirms it is real and that they may use it. It can appear as on-screen text attributed as typed. It is never voiced as if the narrator were its author, and no person is drawn as its author.
- **Never, whatever the brief says:** an invented testimonial, review, rating, customer count, award or press mention; a deadline or scarcity the brief does not state; a drawn person presented as a real customer, doctor or expert; another brand's logo; a competitor's ad copy. The ad templates carry these rules in a fixed instruction after the template text, so editing a template cannot remove them.

## Angles

- **Suggestion.** One job per product and channel: Claude proposes five angles from the sheet (confirmed claims only), the channel (language, country, persona, niche) and the framework catalog; then the decision engine ranks them.
- **Ranking.** Three typed reasons, each a 0 to 100 score with a confidence: *audience fit* (does it speak to the segment it names), *hook* (would the first two seconds hold that viewer) and *clarity* (does a viewer know what the product is and what to do next). Priority weighs hook 40%, audience fit 35% and clarity 25%, in code like the theme ranking's weights. Bardo itself marks an angle that leans on a sensitive claim; that is not asked of the engine.
- **Stored as themes.** An ad angle is a theme whose subject is a product: its niche is the product's category from the sheet, and its title and angle feed the same template variables as any theme. So every later stage, export and metric works unchanged. Ad angles show on the Products screen, not on Themes, and ad projects are left out of the past performance the theme ranking reads.
- Editing an angle drops its ranking until it is ranked again, as for themes.

## The ad script and the claims check

- **A new template kind, *Ad script*,** with the product, the audience segment, the angle, the framework and its steps, the numbered confirmed claims of the Brief stage, the offer, the call to action, the must-say and never-say notes, and the usual channel variables. It keeps the storyboard script's rules (#258): one continuous line, sentences that can be shown.
- **Lengths.** A new length, 15 seconds (about 30 to 38 words), joins 30 seconds, 1 minute and 3 minutes for every project. An ad project starts at 30 seconds.
- **What it writes.** The hook in the first two seconds; then the framework's steps; the product named by the middle at the latest; the call to action last, in the user's words. Only listed claims; no superlative, number, deadline or comparison that is not one of them; no competitor named unless a confirmed comparison names it.
- **Concept products.** The script speaks of an idea: what it would do and why it matters, and a call to action that fits something that does not exist yet (follow, join the list, tell us what you think). No price and no "buy now" unless the brief's own call to action says so.
- **Claims check.** Runs with every generated ad script and, on request, after the user's edits (a small paid call with its estimate). It lists the script's statements about the product, each tied to a confirmed claim or flagged *unbacked*; the Script stage marks the flagged sentences. Flags do not lock later stages. Starting a narration with unbacked statements says so beside the estimate, and the upload review and export make the user acknowledge them.

## Scenes of an ad

Built on the storyboard plan (#258) and visual styles (#259).

- **A new template kind, *Ad scene plan*.** Scenes run 2 to 4 seconds. Each has a kind. The first scene shows the hook. The product is on screen by the time the narration names it. At least one scene is a product shot. The last scene is the end card, over the call to action. The plan may not ask for other brands, logos or real people; the product's own name and look are allowed.
- **Product shots (free).** Composed by Bardo, in the frame's shape, from one product image:
  - a photo with transparency, or one Bardo can cut out (see the spike), stands on the backdrop with a soft shadow;
  - any other photo sits on the backdrop as a card with rounded corners;
  - a screen sits inside a plain device outline drawn by Bardo (a phone for a tall screenshot, a browser window for a wide one), with no maker's marks.
  The image is scaled to fit, never stretched, cropped or redrawn. A warning shows when it is too small for the frame. The scene's motion (zoom, pan) is the cut's, as for any still.
- **Backdrops.** Plain by default, in colors taken from the product image and adjustable. A drawn backdrop is one scene image's price: the image generator draws an empty surface or set in the video's style, and the drawing waits for review like any redrawn scene.
- **Product in use (paid, checked).** Only for photos. The scene's image request carries the product image as an object reference, the product's look from the sheet, and a fixed sentence: keep the product's shape, colors, label and proportions exactly as in the reference. References go in this order within the image model's limit: cast portraits, product, place, previous shot, look reference. The picture then waits for the **product check**: the scene inspector shows it beside the real image, and the user ticks it, draws it again, or picks *Use the real image instead*, which turns the scene into a product shot at no cost.
- **Screens are never drawn.** For a digital product, drawn scenes show people and devices with the screen unreadable; the interface appears only in product shots, from the real screenshot.
- **Clips.** Product shots and the end card are never sent to a video model, in any production mode, and the whole-video estimate counts them as free. A product-in-use scene is animated only after its product check, with a fixed sentence added to its motion prompt (the product stays rigid and unchanged; the camera or the person moves), and its clip waits for a product check of its own.
- **Plans of other projects** are untouched: scenes without a kind are drawn scenes.

## Cutting a product out of its photo (spike)

A product shot looks best when the product stands free of its photo's background. The spike finds out whether Bardo can do that on the computer, free and offline, in the `vision` crate (ONNX Runtime, ADR-0024): a salient-object model under a license that allows commercial use (candidates: U²-Net, IS-Net, BiRefNet in its small sizes; RMBG is out for its license), measured on product photos (glass, reflections, white on white, fine edges) for quality, size and speed on the CPU and with DirectML. The result decides whether the cutout is built or the card layout stays the only one for opaque photos. No slice waits on it: the card layout always works.

## Concept products

- With no real image, *Draw a concept* has Claude write a prompt from the description (a new template kind, with a fixed instruction: an original product, no existing brand, logo, trade dress or text) and the image generator draw it. The drawing waits for review and can be drawn again; the kept one is the product's concept image, used where a photo would be (reference and product shots).
- The project shows a *Concept* badge. Its script follows the concept rules above. The upload review and the export show a notice the user acknowledges: the video shows an idea, not a product that exists.
- When the product gets a real image, new videos use it and are no longer concepts; videos already made keep their badge.

## End card

- Composed by Bardo, like a thumbnail (ADR-0021): a backdrop, one product image, a headline (the call to action, up to 40 characters), a second line (the link or handle, up to 60), an optional third line (the offer, up to 40) and the optional logo. Text is drawn with Bardo's own fonts in one of the thumbnail looks. The user edits each line, the image, the backdrop, the look and where things sit.
- It is the plan's last scene: on screen from the call to action's first word until 1.5 seconds after the narration's last, and for 2.5 seconds at least.
- An offer line comes from the Brief stage, so it can only state a confirmed offer claim.
- Composed for each frame shape the project is rendered in.

## Publishing an ad

- **Metadata.** A new template kind, *Ad metadata*: the caption opens with the hook, uses only the Brief stage's claims, ends with the call to action, and includes the link where the network shows links in the text (YouTube's description, X); where it does not (Instagram, TikTok), it says where the link is, in the user's words from the brief.
- **Commercial disclosure.** The project's default comes from whose the product is: *mine* gives *your brand*, *a client's* gives *paid partnership*. The upload review shows it per network and the user can change it there; a change needs a new review, as for the other review fields. What each network takes (checked against each network's documentation on 2026-11-25; the slice checks again):

| Network | Your brand | Paid partnership |
| --- | --- | --- |
| YouTube | Nothing to set. | The API reads the paid-promotion box (`paidProductPlacementDetails.hasPaidProductPlacement`) but does not list it among the fields an upload can set. The review says to tick *Includes paid promotion* in YouTube Studio, and the publication links to the video's Studio page. If the documentation lists it as settable when the slice starts, Bardo sets it. |
| TikTok | The draft goes to the inbox, whose endpoint has no disclosure field. The review says to turn on *Disclose post content* and pick *Your brand* in TikTok before posting. | Same, with *Branded content*, which TikTok does not allow on a private post. |
| Instagram Reels | Nothing to set. | `is_paid_partnership` on the media container; optionally up to two brand accounts (`branded_content_sponsor_ids`) typed by the user. A refusal shows Meta's reason and nothing is resent without the label. |
| X | Nothing to set. | `paid_partnership` on the post. |
| Kick, and every export | A reminder line in `metadata.txt`. | A reminder line in `metadata.txt`. |

- **Synthetic-content disclosure.** Unchanged, with one more default for ad projects: on when a drawn scene shows the product in a photographic style (or in a style Bardo cannot tell, the channel's free notes), since that is a realistic picture of a real thing that was not filmed. Editable in the review, as today.
- **Review additions.** The upload review and the export of an ad project list the sensitive claims the script uses, the unbacked statements left, the sensitive-category notice and the concept notice, each acknowledged before the upload or export starts. The acknowledgements are kept with the publication.

## Placement exports

- From the Publish stage of an ad project, *Export for ads* renders the cut in the chosen shapes: 4:5 (1080 × 1350) and 1:1 (1080 × 1080), beside the project's own 9:16 or 16:9.
- These are render shapes, not editor shapes: the cut stays in its frame. For each shape, composed pictures (product shots, end card) are composed again for it, so the product and the text are never cropped; drawn pictures and clips fill the frame from a window, centered unless the user moves it per clip in the export's review; captions are laid out for the shape.
- The export's review shows each shape before rendering, as a render review does.
- Output: `Videos\Bardo\<project title> (<id>)\Ads\<shape>\` with the video (H.264, AAC, the loudness of the project's own render) and `ad.txt` holding the primary text, a headline, the call to action, the link and the review's reminders.
- No upload: the user takes the files to the ad manager.

## Costs

Paid: research (once per product), the sheet, angles and their ranking, the script and its claims check, the narration, drawn scenes and drawn backdrops, concept images, clips as the production mode says, music, metadata. Free: products, images, plain backdrops, product shots, the end card, product checks, placement exports.

For scale, a 30-second ad on the rate table (checked 2026-11-25), about ten scenes of which two product shots and the end card are composed: about seven drawn images and a cast portrait at about US$ 0.10 each, a few cents of narration and music, and US$ 0.15 to 0.30 of text. That is about US$ 1.10 to 1.30 in Economy, and about US$ 0.60 more with the hook animated (three 5-second clips on Kling 2.6 at 720p). The whole-video estimate (#248) shows the real figure before anything is generated.

## Not in this feature

- Uploading to or reading from ad managers (Meta, TikTok, Google), campaign budgets, targeting, or ad performance.
- Testimonial ads, and any person presented as a real customer or expert.
- Variants of one ad made automatically for testing. The user can approve several angles; each is its own project.
- Product pictures drawn in place of a real image the user has. The real image wins wherever the product is shown alone.
- Importing product data from store listings or feeds. The link is read once, for research.
- Removing or replacing a background with a paid model. The cutout is local or it is the card.
- Legal review of claims. Bardo keeps what the user confirmed and reminds; the decision is the user's.

## Acceptance

- From a brief and one photo, the user reaches a rendered 30-second ad whose product shots and end card show the photo undistorted, whose script uses only confirmed claims, and whose upload review shows the right disclosure for each network.
- From a description alone, the user reaches a concept ad that never says the product exists, with the concept notice acknowledged before upload.
- A found claim never reaches a script before the user confirms it; a rejected one never does.
- No product shot, end card or screenshot is ever sent to an image or video model, and no scene showing the product is animated or rendered before its product check.
- Research never exceeds its search and fetch limits, is paid once per product, and every found claim names its source.
- An unbacked statement or a sensitive claim in the script is acknowledged in the upload review and written in the export's reminders.
- Every paid call is estimated, recorded in Costs and passes the budget and the video's spend cap.
- Projects that are not ads behave exactly as before.

## Slices

In order (each is a sub-issue of #272). All start after #258 and #259 are merged; the spike can run at any time.

1. #273 Products: a library of things to advertise, with brief and images
2. #274 Product research on the web, and the product sheet with claims the user confirms (after 1)
3. #275 Ad angles: suggested, ranked, and approved into an ad project with its Brief stage (after 2)
4. #276 Ad script from confirmed claims, with a claims check and a 15-second length (after 3)
5. #277 Spike: cut a product out of its photo on the computer
6. #278 Ad scene plan with product shots composed from the real image (after 4)
7. #279 The product as a reference in drawn scenes, checked before clips and render (after 6)
8. #280 Concept products: a drawn concept image for an idea that does not exist yet (after 6)
9. #281 End card: call to action, link and offer composed over the product (after 6)
10. #282 Commercial disclosure per network, ad metadata and the ad's upload review (after 4)
11. #283 Placement exports: the cut in 4:5 and 1:1 for ad managers (after 9)
