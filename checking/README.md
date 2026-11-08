# checking

GUI application for iRacing fuel strategy. It uses `egui` and builds on Windows
and Linux.

## Functions

- Receives telemetry from the sender on TCP port 5555.
- Calculates fuel use for each lap with the LOW, AVG, PUSH and MAX strategies.
- Shows a fuel use chart with a line for each strategy.
- Shows a pit stop table with the pit lap and the fuel to add.
- Shows a fuel table for each lap, with a color for each strategy.
- Shows a warning when you are one or more laps behind the leader.

## Status banner

The banner on the fuel page shows if the fuel is sufficient to the end of the
race:

- **PUSH FUEL OK**: The fuel is sufficient at AVG and at PUSH pace.
- **PUSH MARGINAL**: The fuel is sufficient at AVG pace, but not at PUSH pace.
- **REFUEL REQUIRED**: You must pit. The banner shows the fuel to add for AVG
  and for PUSH.

## Build

```bash
cargo build -p checking --release
```

For the Windows builds, see the recipes in the `justfile` at the top of the
repository.

## Run

```bash
cargo run -p checking --release
```

The application listens for telemetry on `0.0.0.0:5555`.

## Strategies

The calculations use only eligible laps. These laps are not eligible:
- Laps with a yellow flag
- Laps with pit activity
- The first lap of the race, and the first lap after a yellow flag
- Statistical outliers, more than 2 standard deviations from the mean

### AVG

The average of the last eligible laps. The count is 10% of the race laps, with
a minimum of 5. Use it for your usual race pace.

### PUSH

The average of the eligible laps with the highest fuel use. The count is 5% of
the race laps, with a minimum of 3. Each PUSH lap uses the AVG fuel or more. Use
it to make sure that the fuel is sufficient at full pace.

### LOW

The average of the eligible laps with the lowest fuel use. The count is 5% of
the race laps, with a minimum of 3. Each LOW lap uses the AVG fuel or less. Use
it when you save fuel.
