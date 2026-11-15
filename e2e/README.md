# e2e

Inputs of the [12-factor](https://github.com/hop-top/spec-12fc) conformance gate: what bar's command line is meant to do, written down for the grader. How to update and grade them: [Conformance](../CONTRIBUTING.md#conformance).

- [`stories/`](stories/): one YAML story per user flow (render the status line, install it, check a config, export and import it, preview with sample data, subagent rows, the update check, the web plugin manifest, config changes from a script): the intent, the preconditions and the `bar` commands of each step. After changing a command, update its story.
- [`conformance/scenarios/bar/agent-config-contract/1.0.0/scenario.yaml`](conformance/scenarios/bar/agent-config-contract/1.0.0/scenario.yaml): the graded scenario: steps run against `bar` and assertions on their exit codes and output. It pins its story by sha256 (`content_hash`); update the hash after editing [`stories/agent-config-contract.yaml`](stories/agent-config-contract.yaml).

Check the stories and grade locally:

```sh
kit conformance verify-stories
kit conformance verify-no-leak --audit
cargo build && scripts/conformance-grade   # exit 0 on pass, 68 on fail
```

The `12fc` workflow grades every pull request into `next` and every push to it ([scripts/conformance-grade](../scripts/conformance-grade)).
