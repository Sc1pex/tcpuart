import os
Import("env")


def load_env_file(path):
    values = {}
    if not os.path.isfile(path):
        print(
            f"WARNING: '{path}' not found. Copy .env.example to .env and fill in your secrets.")
        return values
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, val = line.partition("=")
            values[key.strip()] = val.strip().strip('"').strip("'")
    return values


secrets = load_env_file(os.path.join(env.subst("$PROJECT_DIR"), ".env"))

for key, val in secrets.items():
    env.Append(CPPDEFINES=[(key, env.StringifyMacro(val))])
    print(f"Loaded secret: {key}")
