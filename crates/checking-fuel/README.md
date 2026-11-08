# checking-fuel

Library for the fuel strategy calculations. The `checking` GUI and the
`checking-iracingio` crate use it.

The `AppState` type does these calculations:
- Fuel use for each lap
- Green and yellow flag detection
- Pit stop detection
- Fuel use for the AVG, PUSH and LOW strategies
- Removal of statistical outliers
- Selection of the laps for each strategy
