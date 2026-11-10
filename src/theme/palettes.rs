//! Per-theme palette data. Role colors are derived from these values.
use super::ThemeId;

pub(super) struct ThemeDef {
    pub(super) id: ThemeId,
    pub(super) is_light: bool,
    // Base palette (12 colors)
    pub(super) crust: [u8; 3],
    pub(super) mantle: [u8; 3],
    pub(super) base: [u8; 3],
    pub(super) surface0: [u8; 3],
    pub(super) surface1: [u8; 3],
    pub(super) surface2: [u8; 3],
    pub(super) overlay0: [u8; 3],
    pub(super) overlay1: [u8; 3],
    pub(super) subtext0: [u8; 3],
    pub(super) subtext1: [u8; 3],
    pub(super) text: [u8; 3],
    // Accent colors (6)
    pub(super) blue: [u8; 3],
    pub(super) green: [u8; 3],
    pub(super) red: [u8; 3],
    pub(super) yellow: [u8; 3],
    pub(super) mauve: [u8; 3],
    pub(super) teal: [u8; 3],
    /// Signature accent. Distinct from `blue` when the theme's accent is another hue.
    pub(super) accent: [u8; 3],
    // ANSI 16 colors
    pub(super) ansi: [[u8; 3]; 16],
}

pub(super) fn all_defs() -> Vec<ThemeDef> {
    vec![
        // 0: Catppuccin Mocha (default)
        ThemeDef {
            id: ThemeId::CatppuccinMocha,
            is_light: false,
            crust: [17, 17, 27],
            mantle: [24, 24, 37],
            base: [30, 30, 46],
            surface0: [49, 50, 68],
            surface1: [69, 71, 90],
            surface2: [88, 91, 112],
            overlay0: [108, 112, 134],
            overlay1: [127, 132, 156],
            subtext0: [166, 173, 200],
            subtext1: [184, 192, 224],
            text: [205, 214, 244],
            blue: [137, 180, 250],
            green: [166, 227, 161],
            red: [243, 139, 168],
            yellow: [249, 226, 175],
            mauve: [203, 166, 247],
            teal: [148, 226, 213],
            accent: [137, 180, 250],
            ansi: [
                [49, 50, 68],    // black (surface0, visible against base)
                [243, 139, 168], // red
                [166, 227, 161], // green
                [249, 226, 175], // yellow
                [137, 180, 250], // blue
                [245, 194, 231], // magenta
                [148, 226, 213], // cyan
                [186, 194, 222], // white (subtext0)
                [108, 112, 134], // bright black (overlay0, ~4.1:1 contrast)
                [255, 159, 188], // bright red
                [186, 247, 181], // bright green
                [255, 246, 195], // bright yellow
                [157, 200, 255], // bright blue
                [255, 214, 246], // bright magenta
                [168, 246, 233], // bright cyan
                [255, 255, 255], // bright white
            ],
        },
        // 1: Dracula
        ThemeDef {
            id: ThemeId::Dracula,
            is_light: false,
            crust: [30, 31, 41],
            mantle: [37, 38, 50],
            base: [40, 42, 54],
            surface0: [55, 57, 72],
            surface1: [68, 71, 90],
            surface2: [83, 85, 108],
            overlay0: [98, 114, 164],
            overlay1: [127, 132, 156],
            subtext0: [174, 178, 194],
            subtext1: [200, 204, 218],
            text: [248, 248, 242],
            blue: [139, 233, 253],
            green: [80, 250, 123],
            red: [255, 85, 85],
            yellow: [241, 250, 140],
            mauve: [189, 147, 249],
            teal: [139, 233, 253],
            accent: [189, 147, 249],
            ansi: [
                [33, 34, 44],    // black
                [255, 85, 85],   // red
                [80, 250, 123],  // green
                [241, 250, 140], // yellow
                [189, 147, 249], // blue
                [255, 121, 198], // magenta
                [139, 233, 253], // cyan
                [248, 248, 242], // white
                [98, 114, 164],  // bright black
                [255, 110, 110], // bright red
                [105, 255, 148], // bright green
                [255, 255, 165], // bright yellow
                [210, 172, 255], // bright blue
                [255, 146, 218], // bright magenta
                [164, 255, 255], // bright cyan
                [255, 255, 255], // bright white
            ],
        },
        // 2: Nord
        ThemeDef {
            id: ThemeId::Nord,
            is_light: false,
            crust: [36, 40, 50],
            mantle: [40, 44, 55],
            base: [46, 52, 64],
            surface0: [59, 66, 82],
            surface1: [67, 76, 94],
            surface2: [76, 86, 106],
            overlay0: [100, 110, 130],
            overlay1: [130, 140, 158],
            subtext0: [160, 170, 185],
            subtext1: [192, 200, 212],
            text: [216, 222, 233],
            blue: [136, 192, 208],
            green: [163, 190, 140],
            red: [191, 97, 106],
            yellow: [235, 203, 139],
            mauve: [180, 142, 173],
            teal: [143, 188, 187],
            accent: [136, 192, 208],
            ansi: [
                [59, 66, 82],    // black
                [191, 97, 106],  // red
                [163, 190, 140], // green
                [235, 203, 139], // yellow
                [129, 161, 193], // blue
                [180, 142, 173], // magenta
                [136, 192, 208], // cyan
                [229, 233, 240], // white
                [76, 86, 106],   // bright black
                [208, 135, 112], // bright red
                [163, 190, 140], // bright green
                [235, 203, 139], // bright yellow
                [136, 192, 208], // bright blue
                [180, 142, 173], // bright magenta
                [143, 188, 187], // bright cyan
                [236, 239, 244], // bright white
            ],
        },
        // 3: Solarized Dark
        ThemeDef {
            id: ThemeId::SolarizedDark,
            is_light: false,
            crust: [0, 34, 43],
            mantle: [0, 38, 48],
            base: [0, 43, 54],
            surface0: [7, 54, 66],
            surface1: [24, 68, 78],
            surface2: [42, 84, 94],
            overlay0: [88, 110, 117],
            overlay1: [101, 123, 131],
            subtext0: [131, 148, 150],
            subtext1: [147, 161, 161],
            text: [238, 232, 213],
            blue: [38, 139, 210],
            green: [133, 153, 0],
            red: [220, 50, 47],
            yellow: [181, 137, 0],
            mauve: [108, 113, 196],
            teal: [42, 161, 152],
            accent: [38, 139, 210],
            ansi: [
                [7, 54, 66],     // black
                [220, 50, 47],   // red
                [133, 153, 0],   // green
                [181, 137, 0],   // yellow
                [38, 139, 210],  // blue
                [211, 54, 130],  // magenta
                [42, 161, 152],  // cyan
                [238, 232, 213], // white
                [0, 43, 54],     // bright black
                [203, 75, 22],   // bright red
                [88, 110, 117],  // bright green
                [101, 123, 131], // bright yellow
                [131, 148, 150], // bright blue
                [108, 113, 196], // bright magenta
                [147, 161, 161], // bright cyan
                [253, 246, 227], // bright white
            ],
        },
        // 4: Gruvbox Dark
        ThemeDef {
            id: ThemeId::GruvboxDark,
            is_light: false,
            crust: [24, 24, 24],
            mantle: [29, 32, 33],
            base: [40, 40, 40],
            surface0: [60, 56, 54],
            surface1: [80, 73, 69],
            surface2: [102, 92, 84],
            overlay0: [124, 111, 100],
            overlay1: [146, 131, 116],
            subtext0: [168, 153, 132],
            subtext1: [189, 174, 147],
            text: [235, 219, 178],
            blue: [131, 165, 152],
            green: [184, 187, 38],
            red: [251, 73, 52],
            yellow: [250, 189, 47],
            mauve: [211, 134, 155],
            teal: [142, 192, 124],
            accent: [131, 165, 152],
            ansi: [
                [60, 56, 54],    // black (surface0, visible against base)
                [204, 36, 29],   // red
                [152, 151, 26],  // green
                [215, 153, 33],  // yellow
                [69, 133, 136],  // blue
                [177, 98, 134],  // magenta
                [104, 157, 106], // cyan
                [168, 153, 132], // white
                [146, 131, 116], // bright black
                [251, 73, 52],   // bright red
                [184, 187, 38],  // bright green
                [250, 189, 47],  // bright yellow
                [131, 165, 152], // bright blue
                [211, 134, 155], // bright magenta
                [142, 192, 124], // bright cyan
                [235, 219, 178], // bright white
            ],
        },
        // 5: Tokyo Night
        ThemeDef {
            id: ThemeId::TokyoNight,
            is_light: false,
            crust: [22, 22, 30],
            mantle: [26, 27, 38],
            base: [36, 40, 59],
            surface0: [52, 56, 78],
            surface1: [65, 72, 104],
            surface2: [86, 95, 137],
            overlay0: [110, 120, 155],
            overlay1: [140, 150, 180],
            subtext0: [160, 170, 195],
            subtext1: [180, 190, 210],
            text: [192, 202, 245],
            blue: [122, 162, 247],
            green: [158, 206, 106],
            red: [247, 118, 142],
            yellow: [224, 175, 104],
            mauve: [187, 154, 247],
            teal: [115, 218, 202],
            accent: [122, 162, 247],
            ansi: [
                [65, 72, 104],   // black
                [247, 118, 142], // red
                [158, 206, 106], // green
                [224, 175, 104], // yellow
                [122, 162, 247], // blue
                [187, 154, 247], // magenta
                [115, 218, 202], // cyan
                [192, 202, 245], // white
                [86, 95, 137],   // bright black
                [255, 148, 168], // bright red
                [178, 226, 126], // bright green
                [244, 195, 124], // bright yellow
                [142, 182, 255], // bright blue
                [207, 174, 255], // bright magenta
                [135, 238, 222], // bright cyan
                [222, 232, 255], // bright white
            ],
        },
        // 6: Rosé Pine
        ThemeDef {
            id: ThemeId::RosePine,
            is_light: false,
            crust: [22, 20, 28],
            mantle: [25, 23, 36],
            base: [35, 33, 54],
            surface0: [42, 39, 63],
            surface1: [57, 53, 82],
            surface2: [72, 68, 100],
            overlay0: [110, 106, 134],
            overlay1: [144, 140, 170],
            subtext0: [165, 160, 190],
            subtext1: [190, 186, 210],
            text: [224, 222, 244],
            blue: [156, 207, 216],
            green: [49, 116, 143],
            red: [235, 111, 146],
            yellow: [246, 193, 119],
            mauve: [196, 167, 231],
            teal: [156, 207, 216],
            accent: [196, 167, 231],
            ansi: [
                [38, 35, 58],    // black
                [235, 111, 146], // red
                [49, 116, 143],  // green
                [246, 193, 119], // yellow
                [156, 207, 216], // blue
                [196, 167, 231], // magenta
                [156, 207, 216], // cyan
                [224, 222, 244], // white
                [110, 106, 134], // bright black
                [255, 131, 166], // bright red
                [69, 136, 163],  // bright green
                [255, 213, 139], // bright yellow
                [176, 227, 236], // bright blue
                [216, 187, 251], // bright magenta
                [176, 227, 236], // bright cyan
                [244, 242, 255], // bright white
            ],
        },
        // 7: One Dark
        ThemeDef {
            id: ThemeId::OneDark,
            is_light: false,
            crust: [27, 29, 34],
            mantle: [33, 37, 43],
            base: [40, 44, 52],
            surface0: [53, 57, 65],
            surface1: [62, 68, 81],
            surface2: [78, 84, 98],
            overlay0: [92, 99, 112],
            overlay1: [115, 120, 133],
            subtext0: [139, 145, 157],
            subtext1: [163, 169, 180],
            text: [171, 178, 191],
            blue: [97, 175, 239],
            green: [152, 195, 121],
            red: [224, 108, 117],
            yellow: [229, 192, 123],
            mauve: [198, 120, 221],
            teal: [86, 182, 194],
            accent: [97, 175, 239],
            ansi: [
                [53, 57, 65],    // black (surface0, visible against base)
                [224, 108, 117], // red
                [152, 195, 121], // green
                [229, 192, 123], // yellow
                [97, 175, 239],  // blue
                [198, 120, 221], // magenta
                [86, 182, 194],  // cyan
                [171, 178, 191], // white
                [92, 99, 112],   // bright black
                [239, 128, 137], // bright red
                [172, 215, 141], // bright green
                [249, 212, 143], // bright yellow
                [117, 195, 255], // bright blue
                [218, 140, 241], // bright magenta
                [106, 202, 214], // bright cyan
                [211, 218, 231], // bright white
            ],
        },
        // 8: Everforest Dark
        ThemeDef {
            id: ThemeId::EverforestDark,
            is_light: false,
            crust: [37, 43, 38],
            mantle: [42, 48, 43],
            base: [47, 53, 47],
            surface0: [62, 68, 62],
            surface1: [78, 84, 78],
            surface2: [93, 100, 93],
            overlay0: [113, 119, 113],
            overlay1: [135, 140, 135],
            subtext0: [157, 163, 157],
            subtext1: [179, 184, 179],
            text: [211, 198, 170],
            blue: [127, 187, 179],
            green: [167, 192, 128],
            red: [230, 126, 128],
            yellow: [219, 188, 127],
            mauve: [214, 153, 182],
            teal: [131, 192, 159],
            accent: [127, 187, 179],
            ansi: [
                [78, 84, 78],    // black
                [230, 126, 128], // red
                [167, 192, 128], // green
                [219, 188, 127], // yellow
                [127, 187, 179], // blue
                [214, 153, 182], // magenta
                [131, 192, 159], // cyan
                [211, 198, 170], // white
                [113, 119, 113], // bright black
                [250, 146, 148], // bright red
                [187, 212, 148], // bright green
                [239, 208, 147], // bright yellow
                [147, 207, 199], // bright blue
                [234, 173, 202], // bright magenta
                [151, 212, 179], // bright cyan
                [231, 218, 190], // bright white
            ],
        },
        // 9: Monokai
        ThemeDef {
            id: ThemeId::Monokai,
            is_light: false,
            crust: [25, 25, 22],
            mantle: [32, 32, 28],
            base: [39, 40, 34],
            surface0: [56, 57, 50],
            surface1: [73, 74, 66],
            surface2: [90, 91, 82],
            overlay0: [117, 113, 94],
            overlay1: [144, 140, 120],
            subtext0: [168, 164, 144],
            subtext1: [192, 188, 168],
            text: [248, 248, 242],
            blue: [102, 217, 239],
            green: [166, 226, 46],
            red: [249, 38, 114],
            yellow: [230, 219, 116],
            mauve: [174, 129, 255],
            teal: [102, 217, 239],
            accent: [102, 217, 239],
            ansi: [
                [56, 57, 50],    // black (surface0, visible against base)
                [249, 38, 114],  // red
                [166, 226, 46],  // green
                [230, 219, 116], // yellow
                [102, 217, 239], // blue
                [174, 129, 255], // magenta
                [102, 217, 239], // cyan
                [248, 248, 242], // white
                [117, 113, 94],  // bright black
                [255, 68, 134],  // bright red
                [186, 246, 66],  // bright green
                [250, 239, 136], // bright yellow
                [122, 237, 255], // bright blue
                [194, 149, 255], // bright magenta
                [122, 237, 255], // bright cyan
                [255, 255, 255], // bright white
            ],
        },
        // 10: GitHub Dark
        ThemeDef {
            id: ThemeId::GitHubDark,
            is_light: false,
            crust: [13, 17, 23],
            mantle: [22, 27, 34],
            base: [36, 41, 47],
            surface0: [48, 54, 61],
            surface1: [56, 62, 71],
            surface2: [72, 79, 88],
            overlay0: [110, 118, 129],
            overlay1: [139, 148, 158],
            subtext0: [163, 172, 182],
            subtext1: [186, 194, 203],
            text: [230, 237, 243],
            blue: [88, 166, 255],
            green: [63, 185, 80],
            red: [248, 81, 73],
            yellow: [210, 153, 34],
            mauve: [188, 140, 255],
            teal: [63, 185, 80],
            accent: [88, 166, 255],
            ansi: [
                [72, 79, 88],    // black
                [255, 123, 114], // red
                [63, 185, 80],   // green
                [210, 153, 34],  // yellow
                [88, 166, 255],  // blue
                [188, 140, 255], // magenta
                [86, 211, 219],  // cyan
                [230, 237, 243], // white
                [110, 118, 129], // bright black
                [255, 148, 139], // bright red
                [86, 211, 100],  // bright green
                [230, 173, 54],  // bright yellow
                [108, 186, 255], // bright blue
                [208, 160, 255], // bright magenta
                [106, 231, 239], // bright cyan
                [255, 255, 255], // bright white
            ],
        },
        // 11: Ayu Dark
        ThemeDef {
            id: ThemeId::AyuDark,
            is_light: false,
            crust: [10, 14, 20],
            mantle: [13, 18, 26],
            base: [15, 20, 30],
            surface0: [28, 34, 46],
            surface1: [40, 48, 64],
            surface2: [55, 65, 82],
            overlay0: [90, 100, 118],
            overlay1: [115, 125, 140],
            subtext0: [140, 150, 165],
            subtext1: [170, 180, 195],
            text: [203, 204, 198],
            blue: [57, 186, 230],
            green: [170, 217, 76],
            red: [240, 113, 120],
            yellow: [255, 180, 84],
            mauve: [210, 166, 255],
            teal: [149, 230, 203],
            accent: [89, 194, 255],
            ansi: [
                [28, 34, 46],    // black (surface0, visible against base)
                [240, 113, 120], // red
                [170, 217, 76],  // green
                [255, 180, 84],  // yellow
                [57, 186, 230],  // blue
                [210, 166, 255], // magenta
                [149, 230, 203], // cyan
                [203, 204, 198], // white
                [90, 100, 118],  // bright black
                [255, 133, 140], // bright red
                [190, 237, 96],  // bright green
                [255, 200, 104], // bright yellow
                [77, 206, 250],  // bright blue
                [230, 186, 255], // bright magenta
                [169, 250, 223], // bright cyan
                [233, 234, 228], // bright white
            ],
        },
        // 12: Material Darker
        ThemeDef {
            id: ThemeId::MaterialDarker,
            is_light: false,
            crust: [25, 25, 25],
            mantle: [30, 30, 30],
            base: [33, 33, 33],
            surface0: [48, 48, 48],
            surface1: [60, 60, 60],
            surface2: [74, 74, 74],
            overlay0: [100, 100, 100],
            overlay1: [130, 130, 130],
            subtext0: [158, 158, 158],
            subtext1: [180, 180, 180],
            text: [238, 255, 255],
            blue: [130, 170, 255],
            green: [195, 232, 141],
            red: [240, 113, 120],
            yellow: [255, 203, 107],
            mauve: [199, 146, 234],
            teal: [137, 221, 255],
            accent: [130, 170, 255],
            ansi: [
                [48, 48, 48],    // black
                [240, 113, 120], // red
                [195, 232, 141], // green
                [255, 203, 107], // yellow
                [130, 170, 255], // blue
                [199, 146, 234], // magenta
                [137, 221, 255], // cyan
                [238, 255, 255], // white
                [100, 100, 100], // bright black
                [255, 133, 140], // bright red
                [215, 252, 161], // bright green
                [255, 223, 127], // bright yellow
                [150, 190, 255], // bright blue
                [219, 166, 254], // bright magenta
                [157, 241, 255], // bright cyan
                [255, 255, 255], // bright white
            ],
        },
        // 13: Solarized Light
        ThemeDef {
            id: ThemeId::SolarizedLight,
            is_light: true,
            crust: [238, 232, 213],
            mantle: [243, 237, 218],
            base: [253, 246, 227],
            surface0: [238, 232, 213],
            surface1: [220, 215, 200],
            surface2: [200, 195, 180],
            overlay0: [147, 161, 161],
            overlay1: [131, 148, 150],
            subtext0: [101, 123, 131],
            subtext1: [88, 110, 117],
            text: [7, 54, 66],
            blue: [38, 139, 210],
            green: [133, 153, 0],
            red: [220, 50, 47],
            yellow: [181, 137, 0],
            mauve: [108, 113, 196],
            teal: [42, 161, 152],
            accent: [38, 139, 210],
            ansi: [
                [7, 54, 66],     // black
                [220, 50, 47],   // red
                [133, 153, 0],   // green
                [181, 137, 0],   // yellow
                [38, 139, 210],  // blue
                [211, 54, 130],  // magenta
                [42, 161, 152],  // cyan
                [238, 232, 213], // white
                [0, 43, 54],     // bright black
                [203, 75, 22],   // bright red
                [88, 110, 117],  // bright green
                [101, 123, 131], // bright yellow
                [131, 148, 150], // bright blue
                [108, 113, 196], // bright magenta
                [147, 161, 161], // bright cyan
                [253, 246, 227], // bright white
            ],
        },
        // 14: Catppuccin Latte
        ThemeDef {
            id: ThemeId::CatppuccinLatte,
            is_light: true,
            crust: [220, 224, 232],
            mantle: [230, 233, 239],
            base: [239, 241, 245],
            surface0: [204, 208, 218],
            surface1: [188, 192, 204],
            surface2: [172, 176, 190],
            overlay0: [140, 143, 161],
            overlay1: [124, 127, 147],
            subtext0: [108, 111, 133],
            subtext1: [92, 95, 119],
            text: [76, 79, 105],
            blue: [30, 102, 245],
            green: [64, 160, 43],
            red: [210, 15, 57],
            yellow: [223, 142, 29],
            mauve: [136, 57, 239],
            teal: [23, 146, 153],
            accent: [30, 102, 245],
            ansi: [
                [172, 176, 190], // black
                [210, 15, 57],   // red
                [64, 160, 43],   // green
                [223, 142, 29],  // yellow
                [30, 102, 245],  // blue
                [136, 57, 239],  // magenta
                [23, 146, 153],  // cyan
                [76, 79, 105],   // white
                [140, 143, 161], // bright black
                [210, 15, 57],   // bright red
                [64, 160, 43],   // bright green
                [223, 142, 29],  // bright yellow
                [30, 102, 245],  // bright blue
                [136, 57, 239],  // bright magenta
                [23, 146, 153],  // bright cyan
                [44, 47, 71],    // bright white
            ],
        },
        // 15: GitHub Light
        ThemeDef {
            id: ThemeId::GitHubLight,
            is_light: true,
            crust: [208, 215, 222],
            mantle: [246, 248, 250],
            base: [255, 255, 255],
            surface0: [234, 238, 242],
            surface1: [208, 215, 222],
            surface2: [175, 184, 193],
            overlay0: [110, 119, 129],
            overlay1: [87, 96, 106],
            subtext0: [89, 99, 110],
            subtext1: [89, 99, 110],
            text: [31, 35, 40],
            blue: [9, 105, 218],
            green: [26, 127, 55],
            red: [207, 34, 46],
            yellow: [154, 103, 0],
            mauve: [130, 80, 223],
            teal: [27, 124, 131],
            accent: [9, 105, 218],
            ansi: [
                [36, 41, 47],    // black
                [207, 34, 46],   // red
                [17, 99, 41],    // green
                [77, 45, 0],     // yellow
                [9, 105, 218],   // blue
                [130, 80, 223],  // magenta
                [27, 124, 131],  // cyan
                [110, 119, 129], // white
                [87, 96, 106],   // bright black
                [164, 14, 38],   // bright red
                [26, 127, 55],   // bright green
                [99, 60, 1],     // bright yellow
                [33, 139, 255],  // bright blue
                [164, 117, 249], // bright magenta
                [49, 146, 170],  // bright cyan
                [140, 149, 159], // bright white
            ],
        },
    ]
}
