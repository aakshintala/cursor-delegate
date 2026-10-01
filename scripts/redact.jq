# Strip the recording user's personal setup from a headless stream line; keep the shape parsers see.
def blank: if type == "array" then [] elif type == "object" then {} else "" end;
def redact: walk(
  if type != "object" then . else
  # pi: system prompt sections (skills, context files, extension prompts) and extension tools (names kept)
    (if (.sections | type) == "object" then .sections |= map_values("[redacted]") else . end)
  | (if (.toolsAdded | type) == "array" then .toolsAdded |= map({name}) else . end)
  # claude: init lists the user's MCP servers, plugins, skills, commands, memory paths
  | (if .type == "system" and .subtype == "init" then
      with_entries(if .key | IN("mcp_servers", "slash_commands", "skills", "plugins", "agents", "memory_paths", "messaging_socket_path")
                   then .value |= blank else . end)
      | if (.tools | type) == "array" then .tools |= map(select(startswith("mcp__") | not)) else . end
     else . end)
  # claude: user hook output
  | (if .type == "system" and .subtype == "hook_response" then .output = "" | .stdout = "" | .stderr = "" else . end)
  end);
