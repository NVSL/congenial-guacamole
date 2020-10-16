# Whiteboard: A collaborative whiteboard with private history for every user written in Rust using Corundum

This collaborative whiteboard uses Corundum to keep the history of actions in the persistent memory for each
individual. Therefore, undoing and redoing actions is always possible. At the heart of the implementation,
there is a WebSocket server that receives commands from clients and performs the operations as well as
recording them in a persistent Key-Value store data structure.

# Dependencies

All dependencies listed in `Cargo.toml` are available except Corundum (`pmem`) which should be obtained
separately. You should download it and change the directory to the repo path. It will be publicly available
after publishing the paper.

To be able to work with the login interface, you need an html server and php. I recommend Apache2. Create
a soft link of `login` folder to `/var/www/html`.

# How to run?

First, change the config file `public_html/server.json` to the following:

```json
{
    "host": "127.0.0.1",
    "port": 3035
}
```

Then, compile and run as follows:

```bash
cargo run
```

This will open a socket at `127.0.0.1:3035`. Now, you can sign up as a user at `http://localhost/login` and start drawing.

Enjoy!
