# rcc

> RCC command-line tool for running automation and managing its environments.
> More information: <https://github.com/joshyorko/rcc>.

- Show the installed RCC version:

`rcc version`

- Show RCC usage recipes:

`rcc docs recipes`

- Create a robot in a new directory using a selected template:

`rcc robot init --directory {{path/to/new_robot}} --template {{template_name}}`

- Resolve a robot environment, allowing downloads and post-install hooks:

`rcc ht vars --robot {{path/to/robot.yaml}} --json`

- Run a named task from a robot configuration:

`rcc run --robot {{path/to/robot.yaml}} --task {{task_name}}`

- Run quick system diagnostics and print JSON:

`rcc diagnostics --quick --json`

- List local Holotree environments as JSON:

`rcc holotree list --json`

- List configured environment artifact providers as JSON:

`rcc provider list --json`
