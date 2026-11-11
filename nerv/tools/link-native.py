import argparse
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("object", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--config", type=Path, default=Path("target/native.json"))
    args = parser.parse_args()
    config = json.loads(args.config.read_text(encoding="utf-8"))
    arguments = []
    for value in config["arguments"]:
        if value == "{native-libs}":
            arguments.extend(config["native_libraries"])
        else:
            arguments.append(value.replace("{root}", config["root"]).replace("{object}", str(args.object.resolve())).replace("{runtime}", config["runtime"]).replace("{output}", str(args.output.resolve())))
    raise SystemExit(subprocess.run([config["linker"], *arguments]).returncode)


if __name__ == "__main__":
    main()
