# Discord-LLM-Bot
Open AI isn't a lot of fun, cuz they're closed source.
What if you could download an open source LLM and talk to it on discord? Killer right?


![LLM Discord Bot: cyberpunk llama](docs/cyberpunk-llama.jpg)

A Discord bot, written in Rust, that generates responses using any language model supported by `llm`.

Built using [llm](https://crates.io/crates/llm).

# How to run/setup:

### 1. Download a model -

First download an llm, which is based on the llama architecture

Download a small model from hugging face  - https://huggingface.co/TheBloke/Llama-2-7B-Chat-GGML/resolve/main/llama-2-7b-chat.ggmlv3.q2_K.bin?download=true
Download a bigger model from gpt4all - https://gpt4all.io/models/gguf/nous-hermes-llama2-13b.Q4_0.gguf

### 2. Move the model to the ‘models’ directory -

Cut paste the model you just downloaded in the **models** directory for your project.

### 3. Update the ***config.toml*** file -
[model]
path = "models/nous-hermes-llama2-13b.Q4_0.gguf"
context_token_length = 2048
architecture = "LLaMA"
prefer_mmap = true
use_gpu = true

In this, change the name of the model in the path variable.

You can also change other things in this config file to customize the running of llm on your machine

### 4. Make a bot on discord and get it’s token -

You can make your bot here - [**https://discord.com/developers/applications**](https://discord.com/developers/applications)

Go in OAuth2, on your bot and copy the ***secret key*** and paste it in ***discord,_token*** of config.toml

and then copy the ***client id*** and paste it in the ***client_id*** of config.toml
[authentication]
discord_token = "xxxxxxxx"
client_id = "xxxxxxx"

### 6. Invite the bot to your discord server -

Go to ***OAuth2/URL Generator***, tick ***bot***  and the select the following permissions - ***Send Messages, Send Messages in Threads, Add Reactions, Read Message History***

Copy the url form below and paste it in a new tab and accept the bot in your server

### 7. Cargo run

Our bot is ready to run!
***cargo run*** the project

We have two commands 
1. hallucinate - this command completes your given prompt according to the llm
2. alpaca - this command is to actually answer your questions

Now, you can run the commands for the bot on your server!