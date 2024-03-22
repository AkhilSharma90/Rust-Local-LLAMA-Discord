use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

// Define the main configuration struct, serializable and deserializable
// Define a structure called Configuration, which holds various configuration settings.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Configuration {
    // Configuration component for authentication settings.
    pub authentication: Authentication,

    // Configuration component for model-related settings.
    pub model: Model,

    // Configuration component for inference-related settings.
    pub inference: Inference,

    // Configuration component for storing commands using a HashMap.
    pub commands: HashMap<String, Command>,
}

// Implement the Default trait for Configuration to provide default values.
impl Default for Configuration {
    // Define the default method to create a new Configuration instance with default settings.
    fn default() -> Self {
        Self {
            // Default settings for authentication.
            authentication: Authentication {
                discord_token: None,
            },

            // Default settings for the model, including file path, 
            // length, architecture, and GPU usage.
            model: Model {
                path: "models/llama-2-7b-chat.ggmlv3.q2_K.bin".into(),
                context_token_length: 2048,
                architecture: llm::ModelArchitecture::Llama.to_string(),
                prefer_mmap: true,
                use_gpu: true,
                gpu_layers: None,
            },

            // Default settings for inference, specifying thread count, 
            // batch size, and update intervals.
            inference: Inference {
                thread_count: 8,
                batch_size: 8,
                discord_message_update_interval_ms: 250,
                replace_newlines: true,
                show_prompt_template: true,
            },

            // Default settings for commands using a HashMap, including two predefined commands.
            commands: HashMap::from_iter([
                (
                    // Default "hallucinate" command with specific characteristics.
                    "hallucinate".into(),
                    Command {
                        enabled: true,
                        description: "Hallucinates some text.".into(),
                        prompt: "{{PROMPT}}".into(),
                    },
                ),
                (
                    // Default "alpaca" command with a more detailed prompt.
                    "alpaca".into(),
                    Command {
                        enabled: true,
                        description: "Responds to the provided instruction.".into(),
                        // The prompt contains a multiline instruction and response template.
                        prompt: indoc::indoc! {
                            "Below is an instruction that describes a task. Write a response that appropriately completes the request.

                            ### Instruction:

                            {{PROMPT}}

                            ### Response:

                            "
                        }
                        .into(),
                    },
                ),
            ]),
        }
    }
}

// Implement additional methods for the Configuration structure
impl Configuration {
    // A constant representing the filename for the configuration file
    const FILENAME: &str = "config.toml";

    // A function to load a configuration from a file
    pub fn load() -> anyhow::Result<Self> {
        // check if reading the file is successful
        let config = if let Ok(file) = std::fs::read_to_string(Self::FILENAME) {
            // If successful, deserialize the file content using the toml crate
            toml::from_str(&file).context("failed to load config")?
        } else {
            // If the file reading fails, create a default configuration, save it, and use it
            let config = Self::default();
            config.save()?; // Save the default configuration
            config // Return the default configuration
        };

        // Return the loaded or default configuration as a Result
        Ok(config)
    }

    // A function to save the current configuration to a file
    fn save(&self) -> anyhow::Result<()> {
        // Write the configuration to the specified file
        Ok(std::fs::write(
            Self::FILENAME,
            toml::to_string_pretty(self)?, // Serialize the configuration to a TOML-formatted string
        )?)
    }
}

// Define a structure to hold authentication settings
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Authentication {
    // Discord token for authentication
    pub discord_token: Option<String>,
}

// Define a structure to hold model-related settings
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Model {
    // Path to the model file
    pub path: PathBuf,
    // Length of the context token
    pub context_token_length: usize,
    // String representation of the model architecture
    pub architecture: String,
    // Preference for memory mapping
    pub prefer_mmap: bool,
    // Whether or not to use GPU support. Note that `llmcord` must be
    // compiled with GPU support for this to work.
    pub use_gpu: bool,
    // The number of layers to offload to the GPU (if `use_gpu` is on).
    // If not set, all layers will be offloaded.
    pub gpu_layers: Option<usize>,
}
// Implementing the additional methods for the Model structure
impl Model {
    // function to parse the model architecture from a string
    pub fn architecture(&self) -> Option<llm::ModelArchitecture> {
        self.architecture.parse().ok()
    }
}

// The structure to hold inference-related settings
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Inference {
    // The number of threads to use
    pub thread_count: usize,
    // When the prompt is sent to the model, it will be batched. This
    // controls the size of that batch. Larger values will result in
    // faster inference, but will use more memory.
    pub batch_size: usize,
    // Low values will result in you getting throttled by Discord
    pub discord_message_update_interval_ms: u64,
    // Whether or not to replace '\n' with newlines
    pub replace_newlines: bool,
    // Whether or not to show the entire prompt template, or just
    // what the user specified
    pub show_prompt_template: bool,
}

// The structure to hold command-related settings
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Command {
    // The flag indicating whether the command is enabled or disabled
    pub enabled: bool,
    // This is the description of the command
    pub description: String,
    // This holds the prompts associated with the command
    pub prompt: String,
}
