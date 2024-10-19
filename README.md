# @ytparse_bot (YouTube Parser Bot)

## Run locally
```
cargo build && RUST_LOG=info TELOXIDE_TOKEN=your_bot_telegram_api_token OPENAI_TOKEN=your_openai_api_organisation_token ./target/debug/ytextractor-rust
```
The command above will run the bot in polling mode, which is enough for development purposes. For production, you should run the bot in webhook mode.

## Run in production
For Mac users, the easiest way to build with Musl using Docker. Linux users might build glibc/libc toolchain.   Then copy the binary to the target host and run it in a minimal docker container.

```
docker run --rm -it -v "$(pwd)":/home/rust/src docker.io/blackdex/rust-musl:x86_64-musl cargo build --release   
cp target/x86_64-unknown-linux-musl/release/ytextractor-rust dist/   
rsync -av ./dist Dockerfile.runtime ssh:user@your_host:/path/to/folder   
```   
   
Build the runtime image for the first time:   
```
cd /path/to/folder
sudo docker build -t ytextractor-runtime -f Dockerfile.runtime .
```   
   
Adjust external ports and envs on your own:       
```
cd /path/to/folder   
sudo docker run --name ytextractor_bot -p 3032:3030 -v $(pwd)/dist/ytextractor-rust:/app/ytextractor-rust -e TELOXIDE_TOKEN=your_bot_telegram_api_token -e OPENAI_TOKEN=sk-svcacct-your_openai_api_organisation_token -e RUST_LOG=info -e ENVIRONMENT=production -e WEBHOOK_URL=https://yourhost --restart unless-stopped -d ytextractor-runtime:latest
```
