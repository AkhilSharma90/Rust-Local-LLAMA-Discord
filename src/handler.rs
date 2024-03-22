use crate::{
    config::{self, Configuration},
    constant,
    generation::{self, Token},
    util::{self, run_and_report_error, DiscordInteraction},
};
use anyhow::Context as AnyhowContext;
use serenity::{
    async_trait,
    builder::CreateComponents,
    client::{Context, EventHandler},
    futures::StreamExt,
    http::Http,
    model::{
        application::interaction::Interaction,
        prelude::{
            command::{Command, CommandOptionType},
            interaction::{
                application_command::ApplicationCommandInteraction, InteractionResponseType,
            },
            *,
        },
    },
};
use std::collections::HashSet;

pub struct Handler {
    // Import necessary dependencies from external crates and modules
    _model_thread: std::thread::JoinHandle<()>, // A handle to the background thread responsible for model generation
    config: Configuration,                      // Holds the configuration settings for the handler
    request_tx: flume::Sender<generation::Request>, // Channel sender for sending requests to the background thread
    cancel_tx: flume::Sender<MessageId>, // Channel sender for canceling a specific message generation
}
// Definition of the Handler struct
impl Handler {
    // Constructor method to create a new Handler instance
    pub fn new(config: Configuration, model: Box<dyn llm::Model>) -> Self {
        // Create unbounded channels for sending requests and cancel messages
        let (request_tx, request_rx) = flume::unbounded::<generation::Request>();
        let (cancel_tx, cancel_rx) = flume::unbounded::<MessageId>();

        // Start a background thread for model generation
        let _model_thread = generation::make_thread(model, request_rx, cancel_rx);

        // Initialize and return a new Handler instance
        Self {
            _model_thread,
            config,
            request_tx,
            cancel_tx,
        }
    }
}

// Implementation of the EventHandler trait for the Handler struct
#[async_trait]
impl EventHandler for Handler {
    //  method called when the bot is ready
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected; registering commands...", ready.user.name);

        // Attempt to register commands, exit with an error if unsuccessful
        if let Err(err) = ready_handler(&ctx.http, &self.config).await {
            println!("Error while registering commands: `{err}`");
            std::process::exit(1);
        }

        println!("{} is good to go!", ready.user.name);
    }

    //  method called when a user interacts with the bot
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        // Reference to the HTTP context for making HTTP requests
        let http = &ctx.http;

        // Match the type of interaction
        match interaction {
            // Handle application command interactions
            Interaction::ApplicationCommand(cmd) => {
                let name = cmd.data.name.as_str();
                let commands = &self.config.commands;

                // Check if the command exists in the configuration
                if let Some(command) = commands.get(name) {
                    // Run the command and report any errors
                    run_and_report_error(
                        &cmd,
                        http,
                        hallucinate(
                            &cmd,
                            http,
                            self.request_tx.clone(),
                            &self.config.inference,
                            command,
                        ),
                    )
                    .await;
                }
            }
            // Handle message component interactions
            Interaction::MessageComponent(cmp) => {
                // Extract information from the custom_id
                if let ["cancel", message_id, user_id] =
                    cmp.data.custom_id.split('#').collect::<Vec<_>>()[..]
                {
                    if let (Ok(message_id), Ok(user_id)) =
                        (message_id.parse::<u64>(), user_id.parse::<u64>())
                    {
                        // Check if the interaction is initiated by the same user
                        if cmp.user.id == user_id {
                            // Send a cancel message to the background thread
                            self.cancel_tx.send(MessageId(message_id)).ok();

                            // Respond with a deferred update to the original message
                            cmp.create_interaction_response(http, |r| {
                                r.kind(InteractionResponseType::DeferredUpdateMessage)
                            })
                            .await
                            .ok();
                        }
                    }
                }
            }
            _ => {} // Ignore other types of interactions
        };
    }
}

//  function to handle the bot's readiness and command registration
async fn ready_handler(http: &Http, config: &Configuration) -> anyhow::Result<()> {
    // Retrieve the globally registered commands from Discord
    let registered_commands = Command::get_global_application_commands(http).await?;

    // Create a HashSet of names from the registered commands
    let registered_commands: HashSet<_> = registered_commands
        .iter()
        .map(|c| c.name.as_str())
        .collect();

    // Create a HashSet of names from the enabled commands in the bot's configuration
    let our_commands: HashSet<_> = config
        .commands
        .iter()
        .filter(|(_, v)| v.enabled)
        .map(|(k, _)| k.as_str())
        .collect();

    // Check if the registered commands match the configured commands
    if registered_commands != our_commands {
        // If there's a mismatch, reset the globally registered commands
        Command::set_global_application_commands(http, |c| c.set_application_commands(vec![]))
            .await?;
    }

    // Iterate over the enabled commands in the bot's configuration
    for (name, command) in config.commands.iter().filter(|(_, v)| v.enabled) {
        // Create a global application command for each configured command
        Command::create_global_application_command(http, |cmd| {
            cmd.name(name)
                .description(command.description.as_str())
                .create_option(|opt| {
                    // Create an option for the prompt parameter
                    opt.name(constant::value::PROMPT)
                        .description("The prompt.")
                        .kind(CommandOptionType::String)
                        .required(true)
                });

            // Create additional parameters for the command
            create_parameters(cmd)
        })
        .await?;
    }

    Ok(()) // Return Ok if the command registration is successful
}

// Function to create additional parameters for an application command
fn create_parameters(
    command: &mut serenity::builder::CreateApplicationCommand,
) -> &mut serenity::builder::CreateApplicationCommand {
    // Create an option for the seed parameter
    command.create_option(|opt| {
        opt.name(constant::value::SEED)
            .kind(CommandOptionType::Integer)
            .description("The seed to use for sampling.")
            .min_int_value(0)
            .required(false)
    })
}

//  function to handle the hallucination process
async fn hallucinate(
    cmd: &ApplicationCommandInteraction,
    http: &Http,
    request_tx: flume::Sender<generation::Request>,
    inference: &config::Inference,
    command: &config::Command,
) -> anyhow::Result<()> {
    // Import constants and utility functions
    use constant::value as v;
    use util::{value_to_integer, value_to_string};

    // Extract options from the command interaction
    let options = &cmd.data.options;

    // Retrieve user prompt from options, converting it to a string
    let user_prompt = util::get_value(options, v::PROMPT)
        .and_then(value_to_string)
        .context("no prompt specified")?;
    println!("user_prompt - {:?}", user_prompt);

    // Replace newlines in the user prompt if specified in the inference configuration
    let user_prompt = if inference.replace_newlines {
        user_prompt.replace("\\n", "\n")
    } else {
        user_prompt
    };

    // Create an Outputter to manage outputting tokens and messages
    let mut outputter = Outputter::new(
        http,
        cmd,
        Prompts {
            show_prompt_template: inference.show_prompt_template,
            processed: command.prompt.replace("{{PROMPT}}", &user_prompt),
            user: user_prompt,
            template: command.prompt.clone(),
        },
        std::time::Duration::from_millis(inference.discord_message_update_interval_ms),
    )
    .await?;

    // Get the interaction message and its ID
    let message = cmd.get_interaction_message(http).await?;
    let message_id = message.id;

    // Retrieve the seed from options, converting it to a u64
    let seed = util::get_value(options, v::SEED)
        .and_then(value_to_integer)
        .map(|i| i as u64);
    println!(" seed - {:?}", seed);

    // Create a channel for communication of tokens
    let (token_tx, token_rx) = flume::unbounded();

    // Send a generation request to the processing thread
    request_tx.send(generation::Request {
        prompt: outputter.prompts.processed.clone(),
        batch_size: inference.batch_size,
        token_tx,
        message_id,
        seed,
    })?;

    // Create a stream from the token receiver
    let mut stream = token_rx.into_stream();

    let mut errored = false;

    // Process tokens from the stream
    while let Some(token) = stream.next().await {
        match token {
            Token::Token(t) => {
                outputter.new_token(&t).await?;
            }
            Token::Error(err) => {
                match err {
                    generation::InferenceError::Cancelled => outputter.cancelled().await?,
                    generation::InferenceError::Custom(m) => outputter.error(&m).await?,
                };
                errored = true;
                break;
            }
        }
    }

    // Finish the outputting process if no errors occurred
    if !errored {
        outputter.finish().await?;
    }

    Ok(()) // Return Ok if the hallucination process is successful
}

// Definition of the Prompts struct
struct Prompts {
    show_prompt_template: bool,
    processed: String,
    user: String,
    template: String,
}

// Implementation of methods for the Prompts struct
impl Prompts {
    // Method to create a markdown message, incorporating user prompt and processed output
    fn make_markdown_message(&self, message: &str) -> String {
        // Determine whether to display the prompt template or the user's actual prompt
        let (message, display_prompt) = if !self.show_prompt_template {
            (self.decouple_prompt_from_message(message), &self.user)
        } else {
            (message.to_string(), &self.processed)
        };

        // Format the message with appropriate markdown styling
        match message.strip_prefix(display_prompt) {
            Some(msg) => format!("**{display_prompt}**{msg}"),
            None => match display_prompt.strip_prefix(&message) {
                Some(ungenerated) => {
                    if message.is_empty() {
                        format!("~~{ungenerated}~~")
                    } else {
                        format!("**{message}**~~{ungenerated}~~")
                    }
                }
                None => message.to_string(),
            },
        }
    }

    // Method to decouple the prompt from the generated output in a message
    fn decouple_prompt_from_message(&self, output: &str) -> String {
        // Split the template into prefix and suffix based on the {{PROMPT}} placeholder
        let (prefix, suffix) = self.template.split_once("{{PROMPT}}").unwrap_or_default();

        // Retrieve the user's prompt
        let prompt = &self.user;

        // Strip the prefix from the generated output
        let message = if let Some(msg) = output.strip_prefix(prefix) {
            msg
        } else {
            return String::new();
        };

        // Strip the user prompt from the remaining message
        let response = if let Some(resp) = message.strip_prefix(prompt) {
            resp
        } else {
            return message.to_string();
        };

        // Strip the suffix from the final response
        let response = if let Some(resp) = response.strip_prefix(suffix) {
            resp
        } else {
            return prompt.to_string();
        };

        // Add a newline if the suffix ends with a newline character
        let newline = if suffix.ends_with('\n') { "\n" } else { "" };

        // Format the decoupled prompt and response
        format!("{prompt}{newline}{response}")
    }
}

// Definition of the Outputter struct
// This code defines a Rust struct named 'Outputter', which is designed to handle the output of a Discord bot interaction.
// this struct manages the output generation process, accumulates generated output,
// handles message chunking, and maintains information about the Discord user and configuration settings for displaying prompts.
// It also has mechanisms to track the state of the output generation and the time of the last update.
struct Outputter<'a> {
    // Reference to the Http client
    http: &'a Http,

    // User ID associated with the Outputter
    user_id: UserId,

    // Vector to store Discord messages
    messages: Vec<Message>,

    // Vector to store message chunks
    chunks: Vec<String>,

    // String to store the concatenated message
    message: String,

    // Struct containing prompts configuration
    prompts: Prompts,

    // Flag indicating if the Outputter is in a terminal state
    in_terminal_state: bool,

    // Instant representing the last update time
    last_update: std::time::Instant,

    // Duration defining the time between updates
    last_update_duration: std::time::Duration,
}

// the <'a> syntax is a lifetime parameter,
// and it's used to specify the lifetime of references within a struct or a function.
// Lifetime parameters are used to ensure that references in the struct are valid for the entire lifetime of the struct.
// This is particularly useful when dealing with references that have a longer or shorter lifetime
// than the struct they are part of. This helps in memory safety
impl<'a> Outputter<'a> {
    // constant defining the maximum size for message chunks
    const MESSAGE_CHUNK_SIZE: usize = 1500;

    // function to create a new Outputter instance
    async fn new(
        http: &'a Http,                            // Reference to Http with lifetime 'a
        cmd: &ApplicationCommandInteraction,       // Discord Application Command Interaction
        prompts: Prompts,                          // Struct containing information about prompts
        last_update_duration: std::time::Duration, // Duration for updating messages
    ) -> anyhow::Result<Outputter<'a>> {
        // Create an interaction response with Discord using a closure
        cmd.create_interaction_response(http, |response| {
            response
                .kind(InteractionResponseType::ChannelMessageWithSource)
                .interaction_response_data(|message| {
                    message
                        .content(format!(
                            "~~{}~~",
                            if prompts.show_prompt_template {
                                &prompts.processed
                            } else {
                                &prompts.user
                            }
                        ))
                        .allowed_mentions(|m| m.empty_roles().empty_users().empty_parse())
                })
        })
        .await?;

        // Get the initial interaction response from Discord
        let starting_message = cmd.get_interaction_response(http).await?;

        // Create and return a new Outputter instance
        Ok(Self {
            http,

            user_id: cmd.user.id,
            messages: vec![starting_message],
            chunks: vec![],

            message: String::new(),
            prompts,

            in_terminal_state: false,

            last_update: std::time::Instant::now(),
            last_update_duration,
        })
    }

    // function to process a new token and update the Outputter
    // processes a new token, accumulates it to the message, and updates message chunks
    async fn new_token(&mut self, token: &str) -> anyhow::Result<()> {
        // Check if the Outputter is in a terminal state
        if self.in_terminal_state {
            return Ok(());
        }

        // If the accumulated message is empty, add the cancellation button to the first message
        if self.message.is_empty() {
            // Add the cancellation button when we receive the first token
            if let Some(first) = self.messages.first_mut() {
                add_cancel_button(self.http, first.id, first, self.user_id).await?;
            }
        }

        // Accumulate the token to the message
        self.message += token;

        // Process the message and split it into chunks
        self.chunks = {
            let mut chunks: Vec<String> = vec![];

            // Convert the message to markdown and split it into words
            let markdown = self.prompts.make_markdown_message(&self.message);
            for word in markdown.split(' ') {
                // If there is a last chunk and it exceeds the maximum size, start a new chunk
                if let Some(last) = chunks.last_mut() {
                    if last.len() > Self::MESSAGE_CHUNK_SIZE {
                        chunks.push(word.to_string());
                    } else {
                        last.push(' ');
                        last.push_str(word);
                    }
                } else {
                    chunks.push(word.to_string());
                }
            }

            chunks
        };

        // if its time to update messages based on elapsed time
        if self.last_update.elapsed() > self.last_update_duration {
            self.sync_messages_with_chunks().await?;
            self.last_update = std::time::Instant::now();
        }

        Ok(())
    }

    // function to handle errors and update the Outputter
    // it handle errors and cancellation, updating the Outputter.
    async fn error(&mut self, err: &str) -> anyhow::Result<()> {
        self.on_error(err).await
    }

    // function to handle cancellation and update the Outputter
    async fn cancelled(&mut self) -> anyhow::Result<()> {
        self.on_error("The generation was cancelled.").await
    }

    // function to finish processing and update the Outputter
    // finishes processing, removes components from messages, and updates based on remaining chunks.
    async fn finish(&mut self) -> anyhow::Result<()> {
        // Edit all messages to remove components
        for msg in &mut self.messages {
            msg.edit(self.http, |m| m.set_components(CreateComponents::default()))
                .await?;
        }

        // Update messages based on the remaining chunks
        self.sync_messages_with_chunks().await?;

        Ok(())
    }

    // function to synchronize messages with chunks. what it does -
    // 1. Updates the content of the last message with the latest chunk.
    // 2. Removes components from existing messages.
    // 3. Creates new messages for remaining chunks and adds a cancel button to the last message
    async fn sync_messages_with_chunks(&mut self) -> anyhow::Result<()> {
        // Update the last message with its latest state, then insert the remaining chunks in one go
        if let Some((msg, chunk)) = self.messages.iter_mut().zip(self.chunks.iter()).last() {
            msg.edit(self.http, |m| m.content(chunk)).await?; // Update the content of the last message
        }

        if self.chunks.len() <= self.messages.len() {
            return Ok(()); // Return if there are no new chunks
        }

        // Remove the cancel button from all existing messages
        for msg in &mut self.messages {
            msg.edit(self.http, |m| m.set_components(CreateComponents::default()))
                .await?; // Remove components from existing messages
        }

        // Create new messages for the remaining chunks
        let Some(first_id) = self.messages.first().map(|m| m.id) else {
            return Ok(()); // Return if there are no existing messages
        };
        for chunk in self.chunks[self.messages.len()..].iter() {
            let last = self.messages.last_mut().unwrap();
            let msg = last.reply(self.http, chunk).await?; // Reply to the last message with new chunk
            self.messages.push(msg); // Store the new message
        }

        // Add the cancel button to the last message
        if let Some(last) = self.messages.last_mut() {
            add_cancel_button(self.http, first_id, last, self.user_id).await?; // Add a cancel button to the last message
        }

        Ok(())
    }

    // function to handle errors and update the Outputter
    // Replaces the content of all messages with strikethrough text
    // Replies to the last message with an error message
    // Sets the terminal state flag to true
    async fn on_error(&mut self, error_message: &str) -> anyhow::Result<()> {
        // Edit all messages to replace content with strikethrough text
        for msg in &mut self.messages {
            let cut_content = format!("~~{}~~", msg.content);
            msg.edit(self.http, |m| {
                m.set_components(CreateComponents::default())
                    .content(cut_content)
            })
            .await?;
        }

        let Some(last) = self.messages.last_mut() else {
            return Ok(()); // Return if there are no messages
        };
        last.reply(self.http, error_message).await?; // Reply to the last message with an error message

        self.in_terminal_state = true; // Set the terminal state flag

        Ok(())
    }
}

// function to add a cancel button to a message
async fn add_cancel_button(
    http: &Http,
    first_id: MessageId,
    msg: &mut Message,
    user_id: UserId,
) -> anyhow::Result<()> {
    // edit the message to include a cancel button
    Ok(msg
        .edit(http, |r| {
            // creates a new set of components with a single action row
            let mut components = CreateComponents::default();
            components.create_action_row(|r| {
                // create a button in the action row
                r.create_button(|b| {
                    b.custom_id(format!("cancel#{first_id}#{user_id}")) // custom identifier for the button
                        .style(component::ButtonStyle::Danger) // style of the button (red/danger)
                        .label("Cancel") // displays label on the button
                })
            });
            r.set_components(components) // sets the created components in the message edit request
        })
        .await?) // Perform the edit operation asynchronously and return the result
}
