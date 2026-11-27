const helpEntries = [
  {
    "id": "basics-1",
    "topic": "basics",
    "q": "What does Sprout Crew do?",
    "a": "You describe a piece of work, such as a proposal, a research brief, a case study or a decision memo, and add your files. Sprout Crew plans the steps, one AI does the writing, and a different AI checks it. You see the plan, the models, what it may do while it works and the most it could cost before you approve it. Nothing runs until you do."
  },
  {
    "id": "basics-2",
    "topic": "basics",
    "q": "Which outcomes can I choose?",
    "a": "Writing and editing, a case study, a proposal, a research brief, social posts, a brainstorm, and a quick brainstorm (a four-step decision memo). App development appears only if you allow projects that write running code (see Running code ). \"Something else\" lets you describe your own steps."
  },
  {
    "id": "basics-3",
    "topic": "basics",
    "q": "Do I need to know anything technical?",
    "a": "No. You need a Mac and an account with an AI provider. OpenRouter is the easiest start: one key gives you many models, including free ones. Everything else is chosen for you, and you can change it later."
  },
  {
    "id": "basics-4",
    "topic": "basics",
    "q": "What is a \"step\", and what is a \"crew\"?",
    "a": "A step is one piece of the work, such as \"Write the first draft\" or \"Check the numbers\". Your crew is the set of roles that do the steps: writers on one side, the second check on the other. Each role has a model you can change."
  },
  {
    "id": "basics-5",
    "topic": "basics",
    "q": "What happens if I remove a step but leave the step that checks it?",
    "a": "The checking step is skipped with a note that says why, and the rest of the run goes on. The plan warns you before you approve. Nothing is charged for a skipped step."
  },
  {
    "id": "basics-6",
    "topic": "basics",
    "q": "What if I start two projects from the same brief?",
    "a": "The second project gets its own name, with a number added, such as harbor-lane-2. A name you type is used as you typed it. If a project already has that name, including an archived one, Sprout Crew says so and nothing is overwritten."
  },
  {
    "id": "basics-7",
    "topic": "basics",
    "q": "What does the second check do?",
    "a": "The second check is a different AI that reads the draft against your brief and your files. For writing and editing, proposals and research briefs it ends with a verdict: Ready, Fix first or Do not send. It lists each problem with the text it means and the fix, and it does not rewrite the piece. The last step is sent that list and asked to make the fixes, and the result says how many notes it was sent, for example Second check: Fix first · 3 notes sent to the final step. A line the check marks as needing no fix is not counted. A long list shows its first three lines, and Show all opens the rest. On a draft with missing details, the fields come first and the check's list sits below them. If the check says Do not send, the last step does not start and the result says Not ready. The result then offers Run the second check again, and Ask for a change can fix the draft first. If the check ends without a verdict, the step shows No verdict in amber with its text folded away, and it does not get a green tick. Other outcomes keep the check they had and show no verdict card. A project made in an earlier version keeps the check it was approved with."
  },
  {
    "id": "basics-8",
    "topic": "basics",
    "q": "What is \"Check what's missing\"?",
    "a": "For writing and editing, proposals and research briefs, the first step of the plan is Check what's missing. It reads your brief and files and lists up to five facts the piece needs that only you can know, such as a price or a date. It runs after you approve the plan, so nothing is sent to an AI before then. If it finds any, the run pauses on a short form headed A few things only you know. An answer it found in one of your files is filled in, with the file it came from, such as From pricing-notes.txt. Change it, type your own, or leave a field empty. Choose Continue to save your answers, or Skip all to go on without any. They are saved as notes the later steps read, so they go to the same AI models as your brief, and you do not approve the plan again. An answer that looks like a password is not saved, and the form asks you to reword it. A question you leave empty shows up in the draft as a missing detail. If nothing is missing, the step finishes and the run goes on with no pause. While the form waits, the Board lists it. A project made in an earlier version keeps the steps it was approved with, so it has no such step."
  },
  {
    "id": "basics-9",
    "topic": "basics",
    "q": "What is under Details on a project?",
    "a": "The project page keeps its top simple: the title, one line saying where the work stands, and what to do next. Under Details are what the project may do while it works, such as asking before it sends or deletes anything, and two meters for progress and cost. Open the Plan & work tab to see the plan itself."
  },
  {
    "id": "basics-10",
    "topic": "basics",
    "q": "How do I ask for a change?",
    "a": "On the Plan & work tab, choose Ask for a change and say what you want, such as dropping a step or checking one more thing. The crew proposes the change and you apply it or choose Not now. A change to the plan needs your approval again before anything more runs."
  },
  {
    "id": "basics-12",
    "topic": "basics",
    "q": "What is the Board?",
    "a": "The Board is the first screen. It lists everything that is waiting for you across your projects, in groups: Needs your approval, Needs an answer, Failed or stopped, and Finished, not reviewed. The riskiest come first. Pick one to read it on the right, and decide it there. A plan shows what will run, what it costs at most, what it reads, what it may do and the exact version you are approving; Approve and start approves that version and runs it, and if anything changed since you read it, nothing is approved and the Board shows the new version. Answers, findings, allowed actions, Try again, Run the rest and Mark reviewed work the same way. Code diffs, external writes, plan changes and missing details open in their project, because they need the whole of what you decide on screen. ⌘⏎ presses the main button, and O opens the project. While one project runs, another project's decisions wait until it ends. Switch the list to Decided to see what you have decided across your projects, newest first: what it was, when, who decided it and the version it covered. J and K move through the list, [ hides it, and E opens or closes every section. If a project could not be read, the Board says so above the list. Finished work stays there until you choose Mark reviewed under Your final document, on Results & sharing. A result that comes later needs reviewing again. An archived project is left off the Board. A finished project is not listed for approval again, because nothing in it is left to run. While work runs, its project and step show at the top of every other screen, with Stop beside them."
  },
  {
    "id": "basics-11",
    "topic": "basics",
    "q": "Can I change how SproutCrew looks?",
    "a": "Yes. In Settings, open Appearance, the first section, and choose a mode (Match this Mac, Light or Dark) and one of five color schemes: Electric, the default, Mono, Pastel, Pop and Lagoon. Each scheme works in light and dark, and keeps every word and status readable. The app icon and the start screen keep the SproutCrew blue and apricot."
  },
  {
    "id": "basics-13",
    "topic": "basics",
    "q": "What is the Policy page?",
    "a": "Policy, in the sidebar, sets what every new project starts with: whether the crew may send anything outside this Mac, delete files or install software (Ask me each time, or Never), and what providers may keep. Running commands stays off, and checking roles only read. A project made before you saved defaults keeps its own policy. The page lists each project as looser, stricter or the same as your defaults, and the sidebar shows a count when one is looser. When you save stricter defaults, SproutCrew offers to tighten the projects that allow more; it never loosens one. Tightening an approved project ends its approval, so you approve its plan again before more steps run, and a running project cannot be changed until it stops. Keeping nothing needs every step on OpenRouter, so a plan takes that on its own plan card."
  },
  {
    "id": "basics-14",
    "topic": "basics",
    "q": "Where do I see what every project has used?",
    "a": "Usage, at the foot of the sidebar, shows today's cost beside it. The page has the limits shared by every project (dollars when a connection has prices, and tokens), the requests a provider never priced, which count at the most they could have cost, and a breakdown by project, day, month, model or role. A project's own limit stays on its Usage & limits tab. G then U opens Usage from anywhere; G then B, P and O open the Board, Projects and Policy."
  },
  {
    "id": "basics-15",
    "topic": "basics",
    "q": "How are my projects listed?",
    "a": "Projects shows two groups: Need you, for a project waiting on an approval or an answer, a failed step, or finished work you have not reviewed, and Going or done for the rest. Each one says its state and how many steps have results; open it to see what it has on the Board, its approval and its limit above its five sections."
  },
  {
    "id": "basics-16",
    "topic": "basics",
    "q": "Where did the Settings tabs go?",
    "a": "Settings is one page of sections, in this order: Appearance, AI connections, Connection checks, Notifications, Projects that write running code, Google Docs, Privacy while recording and About SproutCrew. A closed section still says its state, such as \"none ready \u00b7 4 not set up\" or \"on \u00b7 project names hidden\". A section that needs you, such as a failed connection check, is marked and opens by itself."
  },
  {
    "id": "basics-17",
    "topic": "basics",
    "q": "Why does SproutCrew ask before I remove something?",
    "a": "Removing a source, a note, a template or a saved key asks first, with Keep it already chosen, so a slip of the mouse or Return removes nothing. The question says what happens and what is kept: a removed source is not read by later steps, but results already made keep what they read. Esc also keeps it. Memory is now one page: notes for every project, your saved templates, and where each project's own instructions are."
  },
  {
    "id": "basics-18",
    "topic": "basics",
    "q": "How do I start a project?",
    "a": "Choose New project, or press \u2318N. It is one page: choose what it should make, write the brief, and check the project name, which is suggested from the brief. The buttons under the name open small windows for Files, Rules, Crew, Web research and Models, and each button shows what is set, such as \"Files 3 files\". The line at the bottom says what is still missing, and Create project, or \u2318\u23ce, makes the project. If no AI service is connected yet, the page asks for an OpenRouter key first. Nothing runs until you approve the plan."
  },
  {
    "id": "basics-19",
    "topic": "basics",
    "q": "How are a project's tabs laid out?",
    "a": "Sources, Your crew and Results & sharing are each a page of sections, and a closed section still says its state. Sources has Your sources (with a mark when one tries to instruct the AI), Instructions (the project's rules and notes) and Template; Add a source, at the top, opens one window for files, a web address, another project's results, or GitHub and Notion. Your crew has Brief, Crew, Steps and models, and Tools; Change the plan and Use a ready-made plan open in their own windows, and saving a changed plan means approving it again. Results & sharing has the final document (or the draft), Export, Send it somewhere, and Waiting and sent when a write waits for you."
  },
  {
    "id": "review-board-1",
    "topic": "review-board",
    "q": "What am I approving?",
    "a": "Exactly what the plan card shows: the steps, who writes and who checks, which models they use, what the crew may do while it works, your sources and the limits. If any of these change afterwards, including the address of your own AI service, the approval no longer counts and you approve again. The card names the version it shows, such as 057a 3cf8. If anything in it changes between reading the card and approving, the approval is refused, nothing runs, and the message names the version you read and the current one, so you can read it again."
  },
  {
    "id": "review-board-2",
    "topic": "review-board",
    "q": "What can the crew do while it works?",
    "a": "Under \"While it works\", which sits right under the steps so it is on screen when the plan card opens, the plan card lists in up to six short lines what the crew may do: whether it asks you before sending anything outside this Mac, before deleting files and, when it builds an app, before installing software; that it never runs commands on your Mac; and whether AI providers keep what they are sent. Choose Change to set sending, deleting and installing to ask you each time or never, to see the choices about what providers may keep and about Keynote, and to open the full table of what each role may do (Allow, Ask or Never). Running commands is always off and cannot be turned on. The second check can read the work but never change it."
  },
  {
    "id": "review-board-3",
    "topic": "review-board",
    "q": "What is \"Approve and start\"?",
    "a": "It approves the plan and starts the run in one click; Command-Return does the same. \"Approve only\" approves without starting, so you can start later. Cancel, or Escape, closes the plan without approving anything, and nothing is sent."
  },
  {
    "id": "review-board-4",
    "topic": "review-board",
    "q": "Can the providers keep nothing?",
    "a": "Yes. On the plan, choose Change under \"While it works\", then choose that AI providers keep nothing. Each request then goes only to OpenRouter endpoints that promise not to store prompts or answers. Steps on other providers, and web research, are refused while this is on, and the plan says which."
  },
  {
    "id": "review-board-5",
    "topic": "review-board",
    "q": "Can I change the models or the limit from the plan?",
    "a": "Yes. On the Models line, Change lets you pick another model for the writer or for the second check when those steps run on OpenRouter and you have an OpenRouter key. Only the steps on the model being replaced change, and the plan updates with its new cost. Steps on another provider are changed one step at a time under Adjust the suggested steps, so each keeps its own provider. On the Limit line, Change sets the most paid models may cost this project, higher or lower. Each of these changes the plan, so what you approve is what you see. The plan also says whether the second check is made by a different company from the writer, because a check by the same company is more likely to repeat the same mistake."
  },
  {
    "id": "models-1",
    "topic": "models",
    "q": "Which providers can I connect?",
    "a": "OpenRouter (recommended), Anthropic, OpenAI and Google Gemini, each with your own API key, plus your own AI service at an address you type (next section). Keys are checked with a free request before they are saved."
  },
  {
    "id": "models-2",
    "topic": "models",
    "q": "How do I choose a model?",
    "a": "On a step or role, open the model picker. You can search by name, filter by maker, and see the price in and out per million tokens and how much text the model can read. Recommended models are at the top, then free models, then all of them. If you know the exact model ID, you can type it."
  },
  {
    "id": "models-3",
    "topic": "models",
    "q": "Which models are the defaults?",
    "a": "New projects write with Claude Haiku 5.5 and check with Mistral Small, both through OpenRouter. They are from different makers on purpose, so the check is a second opinion rather than the same model reading its own work."
  },
  {
    "id": "models-4",
    "topic": "models",
    "q": "Will the app switch me to a paid model?",
    "a": "No. The app never moves a step to a paid model unless you choose it and approve the plan."
  },
  {
    "id": "models-5",
    "topic": "models",
    "q": "What are free models' limits?",
    "a": "OpenRouter's free models allow 20 requests a minute and 50 a day, or 1,000 a day once the account has bought $10 of credit. A project with many steps can hit the daily limit. The app waits and retries for the per-minute limit, and tells you plainly when the daily allowance is used up."
  },
  {
    "id": "models-6",
    "topic": "models",
    "q": "Can a model look at my pictures?",
    "a": "Yes, if you choose a model the picker marks as able to read images. Up to four picture sources are shown to it with the request, and they count toward your limits. Text inside a picture is treated as information, never as an instruction."
  },
  {
    "id": "own-service-1",
    "topic": "own-service",
    "q": "Why must the address start with https://?",
    "a": "So what is sent is encrypted on the way. Your brief, sources and results go to this address. The only exception is an app on your own Mac, such as http://localhost:11434 , because nothing leaves the Mac."
  },
  {
    "id": "own-service-2",
    "topic": "own-service",
    "q": "What does it cost?",
    "a": "The app cannot see this service's prices, so cost shows as \"unknown\", never as zero, and the token limit in Usage & limits applies. Check the service's own billing. A model on your Mac costs nothing to run but may write less well than a large hosted model."
  },
  {
    "id": "own-service-3",
    "topic": "own-service",
    "q": "What if I change the address later?",
    "a": "Any key saved for the old address is removed, so it is never sent to the new one, and plans you approved before need a new approval."
  },
  {
    "id": "costs-1",
    "topic": "costs",
    "q": "How do I know what a run will cost?",
    "a": "The plan shows the estimate and the most the run could cost before you approve. During and after a run, Usage & limits leads with one number for the project, such as 3¢ of $5.00, and under Details shows the cost by step, model, role, day and month, and can export it as a spreadsheet file. The menu bar shows today's cost and tokens across all projects."
  },
  {
    "id": "costs-2",
    "topic": "costs",
    "q": "Is there a spending cap?",
    "a": "Paid OpenRouter models stop at $5 per project. Before each request, the app holds the most it could cost against that limit, and refuses a request whose price it does not know. You can change the limit for a project in Usage & limits, up or down, but not below what the project has already spent. There is also a token cap shared by all projects, 500,000 tokens at first. Before you approve, the plan warns you if the run could use more tokens than the cap has left. When a run reaches the cap, AI work holds until you approve a higher cap in Usage; then press Try again, with no need to reopen the app. Set spending controls with your provider too."
  },
  {
    "id": "costs-3",
    "topic": "costs",
    "q": "Why do some costs say \"unknown\"?",
    "a": "When a provider does not report the cost of a request and the app has no price for it, the cost is unknown. The app never counts an unknown cost as zero. On the plan, the app first loads the current model prices, which takes a moment on a new Mac, and says \"Checking prices\" until it has them. If it cannot load them, it says so and asks you to check the connection."
  },
  {
    "id": "costs-4",
    "topic": "costs",
    "q": "What does web research cost?",
    "a": "Web research needs a paid model, and OpenRouter charges a small fee per search (about $0.007 when this was written). The plan includes it in the estimate."
  },
  {
    "id": "costs-5",
    "topic": "costs",
    "q": "What happens if I press Stop?",
    "a": "The run stops and no new step starts. Plan & work says “Stopped by you after step 3” (or whichever step it was), and the Board lists it until you carry on. Choose Run the rest to go on from the next step. A step already sent to a provider cannot be called back, so it finishes and its result is kept. While it finishes, and while any run is going, you cannot open or start another project, and the screen says “Finishing the step that was running…”. The provider can still bill that request, and the app keeps the usage it is told about. Stop is on the run bar, and beside the running project at the top of every other screen. Command-period does the same."
  },
  {
    "id": "sources-1",
    "topic": "sources",
    "q": "What can I add as sources?",
    "a": "Files (PDF, Word, PowerPoint, Excel, text, Markdown and pictures), whole folders including subfolders, public web pages, and Notion pages. Text in a source is information for the crew, never an instruction to it. On a new project they go under Your files, just below the brief. Each one is listed by name with its size (a folder says folder) and a Remove button, and a line such as 2 files and 1 folder added counts them."
  },
  {
    "id": "sources-2",
    "topic": "sources",
    "q": "Can the crew search the web?",
    "a": "Yes, if you turn on web research when you create the project. The research step sees only your brief, never your files, and lists the address of every page it used. Pages are treated as evidence to check, not instructions."
  },
  {
    "id": "sources-3",
    "topic": "sources",
    "q": "How do I make results look like my own documents?",
    "a": "Under Sources → Template , pick one of your Word or PowerPoint files and choose See what it offers . Then tick what to keep: the look (fonts, text size, page size, margins, colour and logo), the section headings, and a short note on its writing style. A PDF gives the sections and style only. You can edit the sections and style, save a template for all projects, or start from one of three: Case study, Proposal and Research brief."
  },
  {
    "id": "results-1",
    "topic": "results",
    "q": "What files do I get?",
    "a": "Word, PDF and PowerPoint versions of the finished work, and an Excel file of the tables in it when it has tables. The PowerPoint is made for every project. If the plan has a deck step, the deck is that step's result. If not, it is made from the document: a slide for each section, up to 12, with up to five short points from the first sentence of each paragraph. Numbers in the spreadsheet are values only, with no formulas. Every result can also be read and copied in the app."
  },
  {
    "id": "results-2",
    "topic": "results",
    "q": "Can the app remove the signs of AI writing?",
    "a": "Yes. For writing, proposals and research briefs, tick Polish the finished text . One more step rewrites the wording only. The app refuses a polished version that changes a number, a heading or the length by much, and keeps the original."
  },
  {
    "id": "results-3",
    "topic": "results",
    "q": "Will I be told when a run finishes?",
    "a": "If you turn on Mac alerts in Settings, you get one when a run finishes, stops, fails or waits on a rate limit. Alerts never include your text, and include the project name only if you allow it."
  },
  {
    "id": "results-4",
    "topic": "results",
    "q": "What is \"How your crews have done\"?",
    "a": "A summary on the Home screen, built from your own projects on this Mac: for each outcome and pair of models, how many runs finished, how many stopped, and how many findings the second check raised. Nothing is sent anywhere."
  },
  {
    "id": "results-5",
    "topic": "results",
    "q": "Why does my result say details missing?",
    "a": "While it writes, the crew marks a fact only you can supply, such as a price or a start date, and never guesses it. The result counts them as Draft · 2 details missing, and the sidebar and the Board say the same. Each one has its own field. Type what you know and choose Finish the article. Your answers are saved as a note and the last step runs once more under the approval you already gave, so nothing new needs approving. On the default models that costs about 1¢. An answer you give is put where its detail was in the text and the files, even if the last step leaves the marker in. If you do not have the details, choose Finish with what I have and the last step writes around them. SproutCrew then takes out any marker the step still left, together with the sentence, paragraph or heading that existed only because of it, so no empty heading or half sentence reaches the files. The result says Finished with 6 details left out and lists them under These were left out of the piece. The list is what you were shown when you chose it, less anything you have answered since, even when the last step had already removed the markers itself. A detail you leave empty stays missing. In the draft view a missing detail shows in the text as [to confirm: …]. Word, PDF and PowerPoint files are made once nothing is missing, or after you finish with what you have, and they never contain it. The step's own result is kept as it was written."
  },
  {
    "id": "results-6",
    "topic": "results",
    "q": "Where are my files?",
    "a": "In your Documents folder, under SproutCrew and then the project's name, for example Documents/SproutCrew/Late freight. On the result, Show in Finder opens that folder. Nothing is ever replaced: if a file with the same name is already there, the new one is saved as name 2, then name 3. A copy is also kept inside the project, and the first time macOS may ask whether SproutCrew can save to Documents. If you say no, the files stay inside the project and the result says so."
  },
  {
    "id": "results-7",
    "topic": "results",
    "q": "What is the run report?",
    "a": "Under Results & sharing, Export run report makes a short summary you can read in a few minutes: each step and the model that did it, what it cost, what the second check said, and which details are still open. It is built from your project's records and no model writes it. The long technical version, with the approved plan, every attempt and every cost entry as JSON, is under Advanced: raw data."
  },
  {
    "id": "notion-1",
    "topic": "notion",
    "q": "Can I read a Notion page into a project?",
    "a": "Yes. Add a Notion page as a source, and its text is read, including the blocks inside it. This needs a Notion integration key saved in your Mac's Keychain, and the page shared with that integration."
  },
  {
    "id": "notion-2",
    "topic": "notion",
    "q": "Can the app write to Notion?",
    "a": "Yes, in two ways. It can make a new page under a page you choose, or add a result to the end of a page you already have, under a heading. Adding never changes or removes what is already on the page. Either way, you see the exact text first, approve it, and then it is sent once."
  },
  {
    "id": "notion-3",
    "topic": "notion",
    "q": "How do I connect Notion?",
    "a": "In Notion, make an integration and copy its key. Then open the page you want to use, choose Connections from its menu, and add your integration. The app reads with one key and writes with a separate one, both kept in the Keychain."
  },
  {
    "id": "notion-4",
    "topic": "notion",
    "q": "Can I read a Google Doc?",
    "a": "Yes. In Settings, choose Connect Google Docs . Google’s sign-in page opens in your browser; choose your account and allow SproutCrew to view your documents. Then, under Sources, choose A Google Doc and paste its link. SproutCrew can only view documents and never edits them. Text, headings, lists and tables are read; pictures are left out. Your sign-in is kept in your Mac’s Keychain, and Disconnect forgets it."
  },
  {
    "id": "notion-5",
    "topic": "notion",
    "q": "Can I save a result into Obsidian or another folder?",
    "a": "Yes. In Results & sharing, type the folder, such as your Obsidian vault. The result is saved as a new Markdown file. An existing file is never replaced; a number is added to the name instead."
  },
  {
    "id": "posts-1",
    "topic": "posts",
    "q": "What does the social posts outcome make?",
    "a": "For each platform you name, such as LinkedIn, X, Instagram, Threads or Bluesky, it writes each post within that platform's limit, with a character count, hashtags, what the picture should show and its size, and alt text. A second AI checks the facts, the tone and the lengths. The result is ready to copy into your scheduling tool or a Notion page."
  },
  {
    "id": "posts-2",
    "topic": "posts",
    "q": "Does it know each platform's current rules?",
    "a": "Turn on web research when you create the project. The first step then looks up each platform's current character limits, image sizes and hashtag norms on the web, with the page each figure came from, and the posts follow them. Without it, the app uses common limits and says they should be checked."
  },
  {
    "id": "posts-3",
    "topic": "posts",
    "q": "Does Sprout Crew post for me?",
    "a": "No. It creates the content; you post it."
  },
  {
    "id": "memory-1",
    "topic": "memory",
    "q": "What does the crew remember?",
    "a": "Only what is on the Memory screen: notes for one project, and notes for every project. You can edit or delete any of them. Every step of every project gets the notes for every project; a project's own notes win if they disagree."
  },
  {
    "id": "memory-2",
    "topic": "memory",
    "q": "Does memory leave my Mac?",
    "a": "Notes are stored on your Mac. They are sent to the AI provider with each request, like the rest of a step's context."
  },
  {
    "id": "code-1",
    "topic": "code",
    "q": "Can Sprout Crew build an app or a prototype?",
    "a": "Only if you turn on Allow projects that write running code in Settings. It is off until you do. Then New project offers App development, and brainstorms, quick brainstorms and proposals can end with a few clickable screens of what is recommended, with sample data. If your sources include screenshots, the prototype shows a before and an after."
  },
  {
    "id": "code-2",
    "topic": "code",
    "q": "Is that safe?",
    "a": "Code is shown to you for review before any later step uses it. A prototype opens only in the app's own preview, which loads nothing from the internet. The crew never runs commands on your Mac."
  },
  {
    "id": "code-3",
    "topic": "code",
    "q": "Can the crew build the deck in Keynote?",
    "a": "Yes, on a Mac with Keynote, including the Keynote that macOS 27 installs as Keynote Creator Studio. When a plan makes slides, the plan card offers Build the deck in Keynote under While it works, Change. Choose Yes and approve the plan. When the files are made, Keynote builds a new deck from the slides, with titles, bullets and presenter notes, saved with the project's other files. It never opens or changes your other files. The deck is then read back and checked against the slides, and any difference is listed. The first time, macOS asks whether Sprout Crew may control Keynote; choose OK. You can change that later in System Settings, Privacy & Security, Automation. The deck uses a clean white Keynote theme. If the project has a template, the deck takes its fonts, its colour for titles and its logo, in the top right corner of each slide, the same as the PowerPoint deck. Text sizes stay Keynote's, because a document's text size is too small on a slide. Pictures from the result itself are not added."
  },
  {
    "id": "code-4",
    "topic": "code",
    "q": "Can the crew use my other apps?",
    "a": "Not yet, apart from building a deck in Keynote. When it comes, it will ask for macOS permission only when you turn it on for a step, never during setup, and the plan will say which app, which file, and how many actions it may take."
  },
  {
    "id": "privacy-1",
    "topic": "privacy",
    "q": "Where are my keys kept?",
    "a": "In your Mac's Keychain. The app reads a key only when it is about to send a request, and never shows it, logs it or sends it anywhere except that provider."
  },
  {
    "id": "privacy-2",
    "topic": "privacy",
    "q": "What is sent to the AI providers?",
    "a": "Your brief and the context each step needs: the relevant sources, earlier results and memory notes. Results are sent outside the Mac, for example to Notion, only when you approve that exact send."
  },
  {
    "id": "privacy-3",
    "topic": "privacy",
    "q": "Does Sprout Crew collect anything about me?",
    "a": "No. There are no analytics. If you report a problem, Settings can make a log file you choose to send, and it never contains your text, file names or keys. See the privacy policy ."
  },
  {
    "id": "privacy-4",
    "topic": "privacy",
    "q": "What if I paste a key into the brief by mistake?",
    "a": "Sprout Crew checks the brief and the project name for text shaped like a key and stops before the project is made, with the message \"This looks like a key. Keys go in the Connect box, not the brief.\" For an OpenRouter key, choose Move it there: the key is checked, saved in your Mac's Keychain and taken out of the brief, and the screen says Connected to OpenRouter. For another kind of key, choose Remove it. If the text only looks like a key, choose It isn't a key and the project is made as written. A key you have already put in a brief that was sent can be seen by that provider, so revoke it where you made it."
  },
  {
    "id": "privacy-5",
    "topic": "privacy",
    "q": "Who is recorded when I approve something?",
    "a": "The person signed in to this Mac: the account's full name, or its login if it has none. SproutCrew reads it itself, so no screen can change it. It is kept in the project's own records, and it appears in a decision log or run report you export. While you record your screen, turn on Hide personal details while I record in Settings: the name then shows as you, and email addresses, your home folder and anything that looks like a key are hidden too. What is stored does not change."
  },
  {
    "id": "trouble-1",
    "topic": "trouble",
    "q": "A step says the free allowance is used up.",
    "a": "OpenRouter's free daily limit is reached. Wait for it to reset, or choose another model for that step with Choose another model , then approve the changed plan."
  },
  {
    "id": "trouble-2",
    "topic": "trouble",
    "q": "A step says my account is out of credit.",
    "a": "Add credit at OpenRouter with Add credit , then choose Try again ."
  },
  {
    "id": "trouble-3",
    "topic": "trouble",
    "q": "A step says the API key has reached its spending limit.",
    "a": "The key itself has a limit, set where the key was made, and it is used up. Nothing was charged for that step. Raise the key's limit with the provider, or connect another key in Settings, then choose Try again . This is separate from the project limit, which you set in the app."
  },
  {
    "id": "trouble-4",
    "topic": "trouble",
    "q": "A step stopped because of the project limit.",
    "a": "The step could have cost more than is left of the project's $5. Raise the limit in Usage & limits, or choose a cheaper model."
  },
  {
    "id": "trouble-5",
    "topic": "trouble",
    "q": "A step says what it sends is too long.",
    "a": "Each step sends its brief, its sources and the results it builds on, and each AI service has a limit on how long that can be. Remove a long source you do not need, or ask for a change that splits the step into smaller ones."
  },
  {
    "id": "trouble-6",
    "topic": "trouble",
    "q": "A provider did not answer within 5 minutes.",
    "a": "The app cannot tell whether that request was charged, so it puts AI work on hold. Check your usage on the provider's website, then quit and reopen Sprout Crew to go on."
  },
  {
    "id": "trouble-7",
    "topic": "trouble",
    "q": "My own AI service cannot be reached.",
    "a": "For an app on your Mac, check it is open and its server is switched on, and that the port number in the address matches. For a hosted service, check the address against its help page; it often ends with /v1 ."
  },
  {
    "id": "trouble-8",
    "topic": "trouble",
    "q": "How do I report a problem?",
    "a": "Use Settings → Export a log for a bug report and email it to hello@sproutcrew.app . Do not send API keys or confidential files."
  }
];
