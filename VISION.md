# VISION.md

## The Thesis

Fawx OS is an agent-native operating environment where the primary interface is intent, not apps.

You do not open an app and navigate a workflow. You tell the system what outcome you want:

- "send this email"
- "cancel that subscription"
- "find a comedy show for date night"
- "show me homes in this price range"
- "send me a curated list of posts I'd actually bookmark"

The system decides how to get it done across browser, phone, messaging, email, calendar, files, maps, and services. The user should think in outcomes. The OS should think in execution.

## The Core Product Idea

The agent is the first-class interaction layer.

That means:

- chat is not an add-on
- voice is not an add-on
- apps are not the main product surface
- navigation is not the primary mode of use

The agent sits above apps, websites, and operating-system affordances and uses them as implementation details in service of user intent.

## What It Should Feel Like

Using Fawx OS should feel like having a fast, competent personal operator living inside your phone.

You give it work. It gets it done.

Sometimes the work is immediate:

- set a reminder
- schedule an event
- send a message
- file a note

Sometimes the work is multi-step:

- compare flight options and suggest the best one
- find comedy shows near me based on comedians I follow
- gather posts on a topic that match my taste
- compare homes in my range and narrow the list

Sometimes the work is annoying and operational:

- cancel a subscription
- navigate a hostile website
- call customer support if the website fails
- keep following up until the task is actually finished

The promise is not "good suggestions." The promise is real completion.

## The Interaction Model

The primary interface is natural-language intent, expressed through:

- typed chat
- voice notes
- images and screenshots
- mixed media and linked context

The system responds in terms of:

- what it is doing
- what it needs from the user
- what it completed
- what remains blocked

The system should prefer:

- outcomes over instructions
- execution over explanation
- concise confirmation over verbose narration

The default successful response is not "here's how to do it." It is "done."

## Background Operation

The agent must be able to work while the user continues using the phone.

This is not a luxury feature. It is part of the core interaction contract.

If the system can only act when it owns the entire foreground experience, it will feel fragile, theatrical, and much less useful than the state of the art. A real agent OS must be able to:

- continue work in the background
- survive app switches
- survive temporary loss of attention from the user
- return with a completed result, a checkpoint, or a clear request for approval

The user should be able to say:

- "cancel that subscription"
- switch to another app
- keep using the phone normally
- come back later and see that the task is done or waiting on a decision

This implies:

- background-safe execution primitives
- explicit checkpoints and resumability
- foreground interruption tolerance
- task state that persists independently of the current UI view

## The Jobs To Be Done

Fawx OS should become excellent at four classes of work.

### 1. Immediate utility

Fast, obvious phone tasks:

- reminders
- events
- messages
- notes
- calls
- alarms
- navigation

### 2. Cross-app execution

Tasks that normally require app hopping:

- sending email
- posting or replying on social platforms
- filing forms
- shopping and booking
- managing subscriptions
- coordinating messages, calendar, and maps together

### 3. Long-horizon planning

Tasks that require memory, judgment, and research:

- planning date nights
- travel planning
- finding local events
- comparing homes
- building shortlists
- collecting and curating information over time

### 4. Annoying real-world operations

Tasks people hate doing:

- cancellations
- customer support calls
- refunds
- follow-up tasks
- repeated retries across channels

If an agent system cannot handle the annoying operational layer, it will always feel like a toy.

## The Bar

The bar is not "better Siri."

The bar is:

- the obvious assistant tasks should be table stakes
- the system should also handle messy, adversarial, long-running, cross-surface work
- the system should actually close loops in the real world

Fawx OS wins when the user says:

"I asked for the result, and it got the result."

## The Trust Model

The user is the principal. The agent is a powerful subordinate operator.

The agent should be:

- highly capable
- fast to act
- explicit about what it is doing
- accountable for what it changed
- unable to violate hard kernel boundaries

Autonomy should be graduated:

- low-risk tasks can execute directly
- medium-risk tasks may require approval
- high-risk tasks require explicit consent and verification

Trust comes from three things:

1. real capability
2. visible accountability
3. hard policy boundaries

Prompt text is not trust. Kernel enforcement is trust.

## The Local / Cloud Model

Local is the default.

The phone should handle as much work as it reasonably can:

- quick tasks
- private tasks
- local state and memory
- immediate device actions

Cloud exists for:

- difficult reasoning
- long-running research
- expensive planning
- workloads that exceed local model limits

Cloud is an escalation path, not the product center.

## What This Is Not

Fawx OS is not:

- a generic chatbot
- a generic agent framework
- an app launcher with AI sugar
- a voice assistant skin
- a desktop workflow tool squeezed onto a phone
- a recommendation engine pretending to be an operator

It is also not a system that hides behind "I couldn't do that."

If the task is possible through browser, phone, messaging, or services, the system should be architected to try to complete it.

## Product Consequences

This vision implies several architectural commitments.

### Browser is first-class

The browser is not an integration. It is a core execution surface.

### Telephony and messaging are first-class

If we want cancellations, support escalation, and real-world task completion, the system must own calls and messaging.

### Long-running execution is core

The system must be able to:

- wait
- retry
- follow up
- resume
- carry work across time
- keep working while the user is doing something else on the device

### Completion must be explicit

"No more tool calls" is not enough. The system must know whether the requested outcome is actually done.

### Audit must be built in

The user must be able to inspect what happened, what changed, and why.

### Apps become implementation details

Apps still matter, but they should no longer define the user experience. The agent experience should sit above them.

### Foreground ownership is not required

The system must not assume that agent work requires exclusive control of the screen at all times.

Some tasks will require active foreground interaction windows. Many should not. The architecture should optimize for work that can proceed safely in the background and recover cleanly when foreground access becomes necessary again.

## The Long Arc

The first implementation may ride on Android as a compatibility substrate.

That is acceptable.

But the long arc is clear:

- less dependence on app-shaped interaction
- less dependence on Java/framework assumptions
- more of the real system owned by a Rust-first runtime
- a phone that behaves like an agentic computer, not a collection of touch UIs

## The Standard For Decisions

When we have to make a product or architecture decision, the test is:

Does this move Fawx OS closer to a world where users express intent and the system gets real work done across the full phone and web environment?

If yes, it is aligned.

If it only makes the UI prettier, the framework thicker, or the implementation easier while weakening that core promise, it is drift.
