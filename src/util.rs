// This file handles all the interactions with the discord API, and is mostly used in the handlers.rs file
use serenity::{
    async_trait,
    http::Http,
    model::{
        prelude::{
            interaction::{
                application_command::{
                    ApplicationCommandInteraction, CommandDataOption, CommandDataOptionValue,
                },
                message_component::MessageComponentInteraction,
                modal::ModalSubmitInteraction,
                InteractionResponseType,
            },
            ChannelId, GuildId, Message,
        },
        user::User,
    },
};
use std::future::Future;

// The Function to get prompt and seed from the discord
pub fn get_value<'a>(
    options: &'a [CommandDataOption],
    name: &'a str,
) -> Option<&'a CommandDataOptionValue> {
    options
        .iter()
        .find(|v| v.name == name)
        .and_then(|v| v.resolved.as_ref())
}

// Function for converting the prompt to a string, so that we can give it to our model
pub fn value_to_string(v: &CommandDataOptionValue) -> Option<String> {
    match v {
        CommandDataOptionValue::String(v) => Some(v.clone()),
        _ => None,
    }
}

// Function for converting the seed from user to integer value
pub fn value_to_integer(v: &CommandDataOptionValue) -> Option<i64> {
    match v {
        CommandDataOptionValue::Integer(v) => Some(*v),
        _ => None,
    }
}

// This is a trait (interface) for Discord interactions with methods for handling the interations with discord
#[async_trait] // This indicates that the trait has asynchronous methods
pub trait DiscordInteraction: Send + Sync {
    // This defines all the methods we are implementing in this trait
    async fn create(&self, http: &Http, message: &str) -> anyhow::Result<()>;
    async fn get_interaction_message(&self, http: &Http) -> anyhow::Result<Message>;
    async fn edit(&self, http: &Http, message: &str) -> anyhow::Result<()>;
    async fn create_or_edit(&self, http: &Http, message: &str) -> anyhow::Result<()>;

    fn channel_id(&self) -> ChannelId;
    fn guild_id(&self) -> Option<GuildId>;
    fn message(&self) -> Option<&Message>;
    fn user(&self) -> &User;
}
// This is the macro for implementing the DiscordInteraction trait.
macro_rules! implement_interaction {
    ($name:ident) => {
        #[async_trait]
        impl DiscordInteraction for $name {
            // This function Creates a new interaction response with the parased message
            async fn create(&self, http: &Http, msg: &str) -> anyhow::Result<()> {
                // We return in Ok(), so we return a Result.
                Ok(self
                    // In Rust, |response| syntax is used to define a function-like construct without explicitly naming it.
                    // Here, the closure, |response| { ... },
                    // takes a parameter named response and contains the logic for creating an interaction response.
                    .create_interaction_response(http, |response| {
                        response
                            .kind(InteractionResponseType::ChannelMessageWithSource)
                            // Here, |message| parameter represents the interaction response data
                            .interaction_response_data(|message| message.content(msg))
                    })
                    .await?)
            }
            // Function to retrieve the existing interaction response as a Message
            async fn get_interaction_message(&self, http: &Http) -> anyhow::Result<Message> {
                Ok(self.get_interaction_response(http).await?)
            }
            // Function to edit the existing interaction response with a new message
            // This allows us to have the typing effect for our bot
            async fn edit(&self, http: &Http, message: &str) -> anyhow::Result<()> {
                Ok(self
                    .get_interaction_message(http)
                    .await?
                    .edit(http, |m| m.content(message))
                    .await?)
            }
            // This function acts as a matcher betweeen the create and edit functions
            // It selects to call the edit function or the create function based on if a respose exists or not
            async fn create_or_edit(&self, http: &Http, message: &str) -> anyhow::Result<()> {
                Ok(
                    if let Ok(mut msg) = self.get_interaction_message(http).await {
                        msg.edit(http, |m| m.content(message)).await?
                    } else {
                        self.create(http, message).await?
                    },
                )
            }

            // Function to get the channel ID associated with the current interaction
            fn channel_id(&self) -> ChannelId {
                self.channel_id
            }
            // Function to get an optional guild id associated with the interaction
            // A guild id refers to the unique identifier for a discord server
            // every server on Discord has a unique id that distinguishes it from other servers
            fn guild_id(&self) -> Option<GuildId> {
                self.guild_id
            }
            // Function to get a reference to the User associated with the interaction
            fn user(&self) -> &User {
                &self.user
            }
            // another macro interaction_message
            // For generating the type of interation
            interaction_message!($name);
        }
    };
}
// This is another macro for implementing for the above macro.
// It implements the message function for the DiscordInteraction trait according to the value passed in it.
macro_rules! interaction_message {
    (ApplicationCommandInteraction) => {
        fn message(&self) -> Option<&Message> {
            None
        }
    };
    (MessageComponentInteraction) => {
        fn message(&self) -> Option<&Message> {
            Some(&self.message)
        }
    };
    (ModalSubmitInteraction) => {
        fn message(&self) -> Option<&Message> {
            self.message.as_ref()
        }
    };
}
// These 3 lines calls the implement_interaction macro for Command Interactions, Message Interactions, and Modal Submit Interactions
implement_interaction!(ApplicationCommandInteraction);
implement_interaction!(MessageComponentInteraction);
implement_interaction!(ModalSubmitInteraction);
// Discord bots and applications,
// Modal Submit Interactions typically refer to interactions involving modals,
// which are graphical user interfaces that overlay the Discord client

// Runs the [body] and edits the interaction response if an error occurs.
pub async fn run_and_report_error(
    interaction: &dyn DiscordInteraction,
    http: &Http,
    body: impl Future<Output = anyhow::Result<()>>,
) {
    if let Err(err) = body.await {
        interaction
            .create_or_edit(http, &format!("Error: {err}"))
            .await
            .unwrap();
    }
}
