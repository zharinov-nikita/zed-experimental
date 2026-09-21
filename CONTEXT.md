# Zed Experimental

Fork-local features for the owner's own use. **Voice Dictation**: the user speaks instead of typing, and the speech becomes the text of a prompt in the AI agent panel composer — the voice never leaves the machine, and the transcript does only if the user points Post-processing at something remote. **Focused Thread**: a way of reading a Thread in which everything the agent did, rather than said, folds out of the way.

## Language

### Dictation flow

**Dictation**:
Voice input into the agent composer. The user's speech becomes prompt text.
_Avoid_: Voice typing, speech input, voice chat

**Dictation Session**:
The span from the moment the user starts dictation until the text is committed or discarded. One session yields one transcript.
_Avoid_: Recording, capture

**Dictation Window**:
A section that unfolds inside the agent panel directly above the Composer during a Dictation Session, pushing the Composer down. All dictated text lives here; nothing reaches the composer until Commit. It never floats.
_Avoid_: Popover, overlay, modal, dialog, floating window

**Commit**:
Placing a Dictation Block into the composer at the cursor. Commit never sends anything to the agent.
_Avoid_: Send, submit, insert, apply

**Dictation Block**:
The result of one Dictation Session as it lives in the composer and in the sent message: a single unit with its own text that can be viewed, removed whole or reopened in the Dictation Window, but is never merged with typed text.
_Avoid_: Chip, crease, attachment, voice note

**Send**:
Delivering the composer contents to the agent. Not part of Dictation; the user does it as usual after Commit.
_Avoid_: Commit, submit

**Discard**:
Ending a Dictation Session without changing the composer.
_Avoid_: Cancel, abort

**Resume**:
Appending new speech to an existing Dictation Block from inside the Dictation Window. Already accepted text stays as it is; only the new part goes through Post-processing.
_Avoid_: Continue, append, re-record

**Session Audio**:
The sound of one Dictation Session as the user spoke it, kept locally so it can be played back or used to reproduce a recognition problem. When the session produced a Dictation Block, a Resume of that block adds its sound to the same Session Audio.
_Avoid_: Recording, WAV, last recording, audio file

### Transcript

**Live Transcript**:
Text of the current Dictation Session shown in the Dictation Window while the user is still speaking. The user always sees what is being recognized.
_Avoid_: Preview, interim text

**Confirmed Text**:
The part of the Live Transcript that will not change anymore.
_Avoid_: Final text, committed text

**Pending Text**:
The tail of the Live Transcript that may still be rewritten as more speech arrives.
_Avoid_: Interim, hypothesis, draft

**Model Loading**:
The wait at the start of a Dictation Session while the Transcription Engine model is read into memory. Shown to the user; nothing is recorded until it is over. Happens once per Zed run unless the user turns off keeping the model loaded.
_Avoid_: Warm-up, initialization, spinner

**Blind Dictation**:
Dictation where the user sees nothing until they stop, as in the Handy app. Explicitly rejected for this feature.
_Avoid_: Dictaphone mode, Handy mode

### Engine and quality

**Transcription Engine**:
The component that turns audio into text. Always local: no network, no API keys, no paid service.
_Avoid_: STT provider, recognizer, cloud engine

**Post-processing**:
Optional rewrite of a transcript driven by a user-editable prompt. May fix punctuation, fillers, term spelling and recognizer artifacts; must not change meaning. It is the only place where recognizer artifacts are removed. The rewriter is the user's choice and need not be local: either a language model the user has configured, or an External Agent, reached through a Post-processing Session.
_Avoid_: Cleanup, polishing, correction

**Post-processing Session**:
The conversation with an External Agent that exists only to rewrite transcripts, held apart from the user's own conversation with the same agent so that neither can see what the other said. It lasts one Dictation Session, so a Resume is rewritten against the text the same session produced a moment earlier, and a later dictation starts from nothing. It is allowed to answer with text and nothing else: if it asks to touch the machine or to ask the user something, the rewrite has failed and the user keeps the raw text.
_Avoid_: Hidden thread, background chat, sub-agent, side session

**Recognizer Artifact**:
A phrase the Transcription Engine invents on silence or noise ("Продолжение следует", "Subtitles by"). The engine never judges it by its words: the Speech Gate keeps silence from being decoded at all, and whatever still slips through is left to Post-processing.
_Avoid_: Hallucination, garbage, noise text

**Speech Gate**:
The Transcription Engine's decision whether the audio it has not yet decoded contains speech at all, made by comparing it with the noise of the same session. While the gate is closed nothing is decoded and the Pending Text stays empty; when it opens, decoding starts from the beginning of the speech, without the silence before it. It opens easily and closes only after clear silence, because a lost word costs more than a stray artifact.
_Avoid_: VAD, energy threshold, silence trimming, volume gate

**Decoder Loop**:
The Transcription Engine repeating one short phrase over and over on a near-empty buffer. A decoding failure, not speech and not a Recognizer Artifact: the engine drops it by its shape, without looking at the words.
_Avoid_: Hallucination loop, stutter, repetition bug

**Engine Assets**:
The files the Transcription Engine cannot start without: the model file and the folder with the backend modules. Their locations are settings; the files themselves come from Engine Download or from anywhere the user put them.
_Avoid_: Weights, binaries, dependencies

**Engine Download**:
Fetching Engine Assets from the Settings window and pointing the settings at them. Chooses from a fixed list; never runs on its own.
_Avoid_: Auto-download, model manager, installer

**Glossary**:
A user-maintained list of terms (mostly English technical words) that guides both the Transcription Engine and Post-processing.
_Avoid_: Vocabulary, dictionary, hints

### Surroundings

**External Agent**:
An AI agent that is not Zed's own: it runs as a separate program with its own account, its own models and its own tools, and Zed only relays what it announces about itself. Zed cannot know its models before talking to it, and does not know what its settings mean.
_Avoid_: ACP agent, CLI agent, external provider, backend

**Composer**:
The text box in the agent panel where the user's prompt is authored before it is sent.
_Avoid_: Message editor, input box, prompt field

**Agent Question**:
A question the agent asks the user in the middle of its work and waits to have answered before it continues. Shown as a card in the agent panel; the user answers there, not in the Composer.
_Avoid_: Elicitation, prompt, form

**Answer Field**:
A text field inside an Agent Question where the user writes the answer. Dictation can go into it the same way it goes into the Composer, but the result is plain text, not a Dictation Block.
_Avoid_: Input, text box, form field

### Quote Reply (adjacent feature, designed after Dictation)

**Quote Reply**:
Replying to a selected fragment of an agent response. The fragment is placed into the Composer as a Quoted Fragment, and the user continues by typing after it or by Dictation, which yields a Quote Reply Block.
_Avoid_: Reply to selection, inline reply, thread reply

**Quoted Fragment**:
The piece of an agent response the user selected for a Quote Reply, taken from the response text only, never from tool cards. Lives in the Composer as a single unit that is removed whole, never merged with typed text.
_Avoid_: Selection, excerpt, citation

**Quote Reply Block**:
A Quoted Fragment and the dictated comment on it, kept together as one unit in the Composer and in the sent message, so it is always clear what the comment refers to. Only the comment can be resumed or edited; the fragment stays as selected.
_Avoid_: Quote with audio, annotated quote, reply chip

### Focused Thread (adjacent feature, designed after Quote Reply)

**Thread**:
Everything one conversation with the agent has produced, shown as a single list in the agent panel: the user's messages, the agent's Speech, and everything the agent did in between.
_Avoid_: Chat, conversation, history, log

**Focused**:
A way of reading a Thread in which every run of work the agent did folds into an Activity, so that Speech, Agent Questions, Permission Requests and failures are all that remain in the list. Its opposite is Full, where the Thread reads as it does upstream.
_Avoid_: Quiet mode, compact mode, clean view, minimal, distraction-free

**Speech**:
What the agent says to the user, as opposed to what it does. Never folded, and its presence is what ends an Activity.
_Avoid_: Text, message, prose, response, output

**Activity**:
One unbroken run of work the agent did without addressing the user, shown in Focused as a single line saying how much of what kind was done, which opens on demand. It is ended by Speech, a user message, an Agent Question, a Permission Request, or a failure — never by the kind of work itself, so a thought between two tool calls stays inside it.
_Avoid_: Group, batch, trace, bundle, collapsed tools

**Permission Request**:
The agent asking to be allowed to run one particular tool call, answered with buttons on that call. Distinct from an Agent Question, which asks the user for information rather than for permission; both are always visible, but only a Permission Request is attached to a single tool call and leaves a permanent mark in the Thread once answered.
_Avoid_: Confirmation, elicitation, approval prompt, Agent Question

**Live Action**:
A tool call the agent is running right now, shown as its own line beneath the Activity it will join, so that a Focused Thread never looks idle while the agent works. There is one line per call in flight, so an agent running several at once shows several.
_Avoid_: Spinner, status line, current tool, progress row
