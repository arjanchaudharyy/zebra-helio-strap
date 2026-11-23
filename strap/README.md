# zebrastrap

A small Python client for the Amazfit Helio Strap over Bluetooth LE. Zebra uses it for the strap features that need an encrypted session.

It is a port of the protocol work in [unstrap](https://github.com/sayed0am/unstrap) (MIT or Apache-2.0).

```sh
python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
export ZEBRA_STRAP_KEY=...      # 32 hex characters; Zebra saves it for you
.venv/bin/python -m zebrastrap.cli info       # name, battery, firmware (no key needed)
.venv/bin/python -m zebrastrap.cli live       # heart rate (no key needed)
.venv/bin/python -m zebrastrap.cli sync       # download history to ./out
.venv/bin/python -m zebrastrap.cli buzz two   # vibrate
.venv/bin/python -m zebrastrap.cli settings   # strap settings as JSON
.venv/bin/python -m zebrastrap.cli set 08:02 --value 140   # change a choice setting
```

Run the tests with `.venv/bin/pip install pytest && .venv/bin/python -m pytest`. The key is never printed.
