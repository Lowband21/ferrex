# Constants Module - Layout and UI Constants

## Modules & Files

### Core Files
- **mod.rs** - Module declaration
- **layout.rs** - All layout-related constants

## Module Structure in layout.rs

### poster
- **BASE_WIDTH**, **BASE_HEIGHT** - Core poster dimensions (200x300)
- **TEXT_AREA_HEIGHT** - Space for title/info below poster
- **TOTAL_CARD_HEIGHT** - Combined height for layout calculations

### animation
- **HOVER_SCALE** - Scale factor for hover effects (1.05)
- **EFFECT_PADDING** - Shadow/glow padding
- **calculate_horizontal_padding()** - Dynamic padding calculation
- **calculate_vertical_padding()** - Dynamic padding calculation
- **DEFAULT_DURATION_MS** - Animation timing

### grid
- **EFFECTIVE_SPACING** - Actual spacing between items (15px)
- **MIN_VIEWPORT_PADDING** - Minimum edge padding (40px)
- **TOTAL_HORIZONTAL_PADDING** - Combined left+right padding
- **MIN_COLUMNS**, **MAX_COLUMNS** - Grid constraints (1-16)
- **ROW_SPACING** - Vertical spacing between rows
- **BOTTOM_PADDING**, **TOP_PADDING** - Grid edge padding

### virtual_grid
- **ROW_HEIGHT** - Calculated row height for virtual scrolling

### scale_presets
- **SCALE_NORMAL** - Normal scale (1.0)
- **DEFAULT_SCALE** - Default user scale

### backdrop
- Various aspect ratio constants for backdrop cropping
- **SOURCE_ASPECT** - Original 16:9
- **DISPLAY_ASPECT** - Target 21:9
- **CROP_FACTOR**, **CROP_BIAS_TOP** - Cropping calculations

### header
- **HEIGHT** - Fixed header height (50px)

### calculations
- Helper functions for dynamic layout calculations
- **calculate_columns()** - Columns per viewport width
- **calculate_grid_padding()** - Center grid with padding
- **get_container_dimensions()** - Size including animation space

## Common Functionality Groupings

### Grid Layout Calculations
- Look in: `calculations` module for dynamic calculations
- Constants: `grid` module for spacing and padding
- Virtual scrolling: `virtual_grid::ROW_HEIGHT`

### Animation Sizing
- Padding functions in `animation` module
- Used by grid calculations to reserve hover space

### Responsive Design
- Column calculation adapts to viewport
- Padding ensures centered layout
- Scale affects all dimensions

## Best Practices

### Constant Organization
- **Group by feature**: Keep related constants together
- **Use modules**: Separate concerns (poster, grid, animation)
- **Document units**: Always specify px, ms, ratios

### Naming Conventions
- **SIZE/DIMENSION**: Use `WIDTH`, `HEIGHT`, `SIZE`
- **SPACING**: Use `PADDING`, `SPACING`, `MARGIN`
- **TIME**: Include `_MS` suffix for milliseconds
- **RATIOS**: Use `FACTOR`, `ASPECT`, `SCALE`

### Usage Patterns
```rust
// Import specific modules
use crate::constants::layout::{poster, grid, calculations};

// Use fully qualified names for clarity
let width = poster::BASE_WIDTH * scale;
let columns = calculations::calculate_columns(viewport_width, scale);
```

### Avoiding Dead Constants
- **Regular audits**: Check for unused constants
- **No speculative constants**: Add only when needed
- **Prefer calculations**: Use functions over many constants

## Cleanup Status
- ✅ Removed unused constants:
  - `CORNER_RADIUS` - Not used anywhere
  - `SCALE_PADDING_PERCENT` - Calculation used directly
  - `DEBUG_DURATION_MS` - Debug feature removed
  - `ITEM_SPACING` - Components define their own
  - `OVERSCAN_ROWS` - Defined in performance_config.rs
- ✅ Removed entire `density_presets` module - Never implemented
- ✅ Removed unused scale presets - Only normal/default used

## Future Considerations
- Scale presets could be re-added when UI settings are implemented
- Density settings might be useful for accessibility
- Consider moving performance constants from performance_config.rs here