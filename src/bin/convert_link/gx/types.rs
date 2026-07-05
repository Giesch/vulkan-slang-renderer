//! Typed enums for every GX byte the converter reads. Parse-don't-validate:
//! every raw value must map to a known variant or the parse fails with the
//! field name and value. Numeric values verified against
//! ../tww/include/dolphin/gx/GXEnum.h; canonical `Display` spellings are the
//! shared vocabulary of the MAT3 diff gate (scripts/link_mat3_table.py prints
//! the same names from gclib's enums).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GxEnumError {
    pub kind: &'static str,
    pub value: u32,
}

impl fmt::Display for GxEnumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {} value {:#x}", self.kind, self.value)
    }
}

impl std::error::Error for GxEnumError {}

/// Declares a GX enum with `TryFrom<u8>` and a canonical `Display` string
/// (the exact spelling the MAT3 oracle prints for the same value).
macro_rules! gx_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $val:literal => $canon:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($variant = $val,)+
        }

        impl TryFrom<u8> for $name {
            type Error = GxEnumError;
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($val => Ok(Self::$variant),)+
                    _ => Err(GxEnumError { kind: stringify!($name), value: value as u32 }),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self {
                    $(Self::$variant => $canon,)+
                })
            }
        }
    };
}

gx_enum! {
    /// GXTexFmt / file-format byte of ResTIMG (GXEnum.h:357–370, 455–457)
    ImageFormat {
        I4 = 0x0 => "I4",
        I8 = 0x1 => "I8",
        Ia4 = 0x2 => "IA4",
        Ia8 = 0x3 => "IA8",
        Rgb565 = 0x4 => "RGB565",
        Rgb5a3 = 0x5 => "RGB5A3",
        Rgba8 = 0x6 => "RGBA32",
        C4 = 0x8 => "C4",
        C8 = 0x9 => "C8",
        C14x2 = 0xA => "C14X2",
        Cmpr = 0xE => "CMPR",
    }
}

gx_enum! {
    /// GXTlutFmt (GXEnum.h:399–401)
    PaletteFormat {
        Ia8 = 0x0 => "IA8",
        Rgb565 = 0x1 => "RGB565",
        Rgb5a3 = 0x2 => "RGB5A3",
    }
}

gx_enum! {
    /// GXTexWrapMode (GXEnum.h:432–434)
    WrapMode {
        Clamp = 0x0 => "ClampToEdge",
        Repeat = 0x1 => "Repeat",
        Mirror = 0x2 => "MirroredRepeat",
    }
}

gx_enum! {
    /// GXTexFilter (GXEnum.h:439–444)
    FilterMode {
        Nearest = 0x0 => "Nearest",
        Linear = 0x1 => "Linear",
        NearestMipNearest = 0x2 => "NearestMipmapNearest",
        LinearMipNearest = 0x3 => "LinearMipmapNearest",
        NearestMipLinear = 0x4 => "NearestMipmapLinear",
        LinearMipLinear = 0x5 => "LinearMipmapLinear",
    }
}

gx_enum! {
    /// J3D material pixel-engine mode (gclib PixelEngineMode)
    PixelEngineMode {
        Opaque = 0x1 => "Opaque",
        AlphaTest = 0x2 => "Alpha_Test",
        Translucent = 0x4 => "Translucent",
    }
}

gx_enum! {
    /// GXCullMode (GXEnum.h:17–20); stored as u32 in MAT3's list
    CullMode {
        None = 0x0 => "Cull_None",
        Front = 0x1 => "Cull_Front",
        Back = 0x2 => "Cull_Back",
        All = 0x3 => "Cull_All",
    }
}

gx_enum! {
    /// GXCompare (GXEnum.h:466–473)
    CompareType {
        Never = 0x0 => "Never",
        Less = 0x1 => "Less",
        Equal = 0x2 => "Equal",
        LessEqual = 0x3 => "Less_Equal",
        Greater = 0x4 => "Greater",
        NotEqual = 0x5 => "Not_Equal",
        GreaterEqual = 0x6 => "Greater_Equal",
        Always = 0x7 => "Always",
    }
}

gx_enum! {
    /// GXAlphaOp (GXEnum.h:477–480)
    AlphaOp {
        And = 0x0 => "AND",
        Or = 0x1 => "OR",
        Xor = 0x2 => "XOR",
        Xnor = 0x3 => "XNOR",
    }
}

gx_enum! {
    /// GXColorSrc (GXEnum.h:92–93)
    ColorSrc {
        Register = 0x0 => "Register",
        Vertex = 0x1 => "Vertex",
    }
}

gx_enum! {
    /// GXDiffuseFn (GXEnum.h:110–112)
    DiffuseFunction {
        None = 0x0 => "None_",
        Signed = 0x1 => "Signed",
        Clamp = 0x2 => "Clamp",
    }
}

gx_enum! {
    /// GXAttnFn (GXEnum.h:116–118)
    AttenuationFunction {
        Specular = 0x0 => "Specular",
        Spot = 0x1 => "Spot",
        None = 0x2 => "None_",
    }
}

gx_enum! {
    /// GXBlendMode (GXEnum.h:147–150)
    BlendMode {
        None = 0x0 => "None_",
        Blend = 0x1 => "Blend",
        Logic = 0x2 => "Logic",
        Subtract = 0x3 => "Subtract",
    }
}

gx_enum! {
    /// GXBlendFactor (GXEnum.h:155–164); src/dst-color aliases share values
    BlendFactor {
        Zero = 0x0 => "Zero",
        One = 0x1 => "One",
        SourceColor = 0x2 => "Source_Color",
        InverseSourceColor = 0x3 => "Inverse_Source_Color",
        SourceAlpha = 0x4 => "Source_Alpha",
        InverseSourceAlpha = 0x5 => "Inverse_Source_Alpha",
        DestinationAlpha = 0x6 => "Destination_Alpha",
        InverseDestinationAlpha = 0x7 => "Inverse_Destination_Alpha",
    }
}

gx_enum! {
    /// GXLogicOp (GXEnum.h:168–183)
    LogicOp {
        Clear = 0x0 => "CLEAR",
        And = 0x1 => "AND",
        RevAnd = 0x2 => "REV_AND",
        Copy = 0x3 => "COPY",
        InvAnd = 0x4 => "INV_AND",
        Noop = 0x5 => "NOOP",
        Xor = 0x6 => "XOR",
        Or = 0x7 => "OR",
        Nor = 0x8 => "NOR",
        Equiv = 0x9 => "EQUIV",
        Inv = 0xA => "INV",
        RevOr = 0xB => "REV_OR",
        InvCopy = 0xC => "INV_COPY",
        InvOr = 0xD => "INV_OR",
        Nand = 0xE => "NAND",
        Set = 0xF => "SET",
    }
}

gx_enum! {
    /// GXTexGenType (GXEnum.h:576–586)
    TexGenType {
        Mtx3x4 = 0x0 => "MTX3x4",
        Mtx2x4 = 0x1 => "MTX2x4",
        Bump0 = 0x2 => "BUMP0",
        Bump1 = 0x3 => "BUMP1",
        Bump2 = 0x4 => "BUMP2",
        Bump3 = 0x5 => "BUMP3",
        Bump4 = 0x6 => "BUMP4",
        Bump5 = 0x7 => "BUMP5",
        Bump6 = 0x8 => "BUMP6",
        Bump7 = 0x9 => "BUMP7",
        Srtg = 0xA => "SRTG",
    }
}

gx_enum! {
    /// GXTexGenSrc (GXEnum.h:590–610)
    TexGenSrc {
        Pos = 0x00 => "POS",
        Nrm = 0x01 => "NRM",
        Binrm = 0x02 => "BINRM",
        Tangent = 0x03 => "TANGENT",
        Tex0 = 0x04 => "TEX0",
        Tex1 = 0x05 => "TEX1",
        Tex2 = 0x06 => "TEX2",
        Tex3 = 0x07 => "TEX3",
        Tex4 = 0x08 => "TEX4",
        Tex5 = 0x09 => "TEX5",
        Tex6 = 0x0A => "TEX6",
        Tex7 = 0x0B => "TEX7",
        Texcoord0 = 0x0C => "TEXCOORD0",
        Texcoord1 = 0x0D => "TEXCOORD1",
        Texcoord2 = 0x0E => "TEXCOORD2",
        Texcoord3 = 0x0F => "TEXCOORD3",
        Texcoord4 = 0x10 => "TEXCOORD4",
        Texcoord5 = 0x11 => "TEXCOORD5",
        Texcoord6 = 0x12 => "TEXCOORD6",
        Color0 = 0x13 => "COLOR0",
        Color1 = 0x14 => "COLOR1",
    }
}

gx_enum! {
    /// GXTexMtx / GXPosNrmMtx (GXEnum.h:729–747); PNMTXn = 3n, TEXMTXn = 30+3n
    TexGenMatrix {
        Pnmtx0 = 0 => "PNMTX0",
        Pnmtx1 = 3 => "PNMTX1",
        Pnmtx2 = 6 => "PNMTX2",
        Pnmtx3 = 9 => "PNMTX3",
        Pnmtx4 = 12 => "PNMTX4",
        Pnmtx5 = 15 => "PNMTX5",
        Pnmtx6 = 18 => "PNMTX6",
        Pnmtx7 = 21 => "PNMTX7",
        Pnmtx8 = 24 => "PNMTX8",
        Texmtx0 = 30 => "TEXMTX0",
        Texmtx1 = 33 => "TEXMTX1",
        Texmtx2 = 36 => "TEXMTX2",
        Texmtx3 = 39 => "TEXMTX3",
        Texmtx4 = 42 => "TEXMTX4",
        Texmtx5 = 45 => "TEXMTX5",
        Texmtx6 = 48 => "TEXMTX6",
        Texmtx7 = 51 => "TEXMTX7",
        Texmtx8 = 54 => "TEXMTX8",
        Texmtx9 = 57 => "TEXMTX9",
        Identity = 60 => "IDENTITY",
    }
}

gx_enum! {
    /// GXTevColorArg (GXEnum.h:294–309)
    CombineColor {
        CPrev = 0x0 => "CPREV",
        APrev = 0x1 => "APREV",
        C0 = 0x2 => "C0",
        A0 = 0x3 => "A0",
        C1 = 0x4 => "C1",
        A1 = 0x5 => "A1",
        C2 = 0x6 => "C2",
        A2 = 0x7 => "A2",
        TexC = 0x8 => "TEXC",
        TexA = 0x9 => "TEXA",
        RasC = 0xA => "RASC",
        RasA = 0xB => "RASA",
        One = 0xC => "ONE",
        Half = 0xD => "HALF",
        Konst = 0xE => "KONST",
        Zero = 0xF => "ZERO",
    }
}

gx_enum! {
    /// GXTevAlphaArg (GXEnum.h:336–343)
    CombineAlpha {
        APrev = 0x0 => "APREV",
        A0 = 0x1 => "A0",
        A1 = 0x2 => "A1",
        A2 = 0x3 => "A2",
        TexA = 0x4 => "TEXA",
        RasA = 0x5 => "RASA",
        Konst = 0x6 => "KONST",
        Zero = 0x7 => "ZERO",
    }
}

gx_enum! {
    /// GXTevOp (GXEnum.h:272–283)
    TevOp {
        Add = 0x0 => "ADD",
        Sub = 0x1 => "SUB",
        CompR8Gt = 0x8 => "COMP_R8_GT",
        CompR8Eq = 0x9 => "COMP_R8_EQ",
        CompGr16Gt = 0xA => "COMP_GR16_GT",
        CompGr16Eq = 0xB => "COMP_GR16_EQ",
        CompBgr24Gt = 0xC => "COMP_BGR24_GT",
        CompBgr24Eq = 0xD => "COMP_BGR24_EQ",
        CompRgb8Gt = 0xE => "COMP_RGB8_GT",
        CompRgb8Eq = 0xF => "COMP_RGB8_EQ",
    }
}

gx_enum! {
    /// GXTevBias (GXEnum.h:287–289) + J3D's 0x3 "compare mode" marker
    TevBias {
        Zero = 0x0 => "ZERO",
        AddHalf = 0x1 => "ADDHALF",
        SubHalf = 0x2 => "SUBHALF",
        HwbCompare = 0x3 => "HWB_COMPARE",
    }
}

gx_enum! {
    /// GXTevScale (GXEnum.h:320–323)
    TevScale {
        Scale1 = 0x0 => "SCALE_1",
        Scale2 = 0x1 => "SCALE_2",
        Scale4 = 0x2 => "SCALE_4",
        Divide2 = 0x3 => "DIVIDE_2",
    }
}

gx_enum! {
    /// GXTevRegID (GXEnum.h:328–331)
    Register {
        Prev = 0x0 => "PREV",
        Reg0 = 0x1 => "REG0",
        Reg1 = 0x2 => "REG1",
        Reg2 = 0x3 => "REG2",
    }
}

gx_enum! {
    /// GXTevKColorSel (GXEnum.h:537–564)
    KonstColorSel {
        One = 0x00 => "_1",
        SevenEighths = 0x01 => "_7_8th",
        SixEighths = 0x02 => "_6_8th",
        FiveEighths = 0x03 => "_5_8th",
        FourEighths = 0x04 => "_4_8th",
        ThreeEighths = 0x05 => "_3_8th",
        TwoEighths = 0x06 => "_2_8th",
        OneEighth = 0x07 => "_1_8th",
        K0 = 0x0C => "K0",
        K1 = 0x0D => "K1",
        K2 = 0x0E => "K2",
        K3 = 0x0F => "K3",
        K0R = 0x10 => "K0_R",
        K1R = 0x11 => "K1_R",
        K2R = 0x12 => "K2_R",
        K3R = 0x13 => "K3_R",
        K0G = 0x14 => "K0_G",
        K1G = 0x15 => "K1_G",
        K2G = 0x16 => "K2_G",
        K3G = 0x17 => "K3_G",
        K0B = 0x18 => "K0_B",
        K1B = 0x19 => "K1_B",
        K2B = 0x1A => "K2_B",
        K3B = 0x1B => "K3_B",
        K0A = 0x1C => "K0_A",
        K1A = 0x1D => "K1_A",
        K2A = 0x1E => "K2_A",
        K3A = 0x1F => "K3_A",
    }
}

gx_enum! {
    /// GXTevKAlphaSel (GXEnum.h:509–533)
    KonstAlphaSel {
        One = 0x00 => "_1",
        SevenEighths = 0x01 => "_7_8th",
        SixEighths = 0x02 => "_6_8th",
        FiveEighths = 0x03 => "_5_8th",
        FourEighths = 0x04 => "_4_8th",
        ThreeEighths = 0x05 => "_3_8th",
        TwoEighths = 0x06 => "_2_8th",
        OneEighth = 0x07 => "_1_8th",
        K0R = 0x10 => "K0_R",
        K1R = 0x11 => "K1_R",
        K2R = 0x12 => "K2_R",
        K3R = 0x13 => "K3_R",
        K0G = 0x14 => "K0_G",
        K1G = 0x15 => "K1_G",
        K2G = 0x16 => "K2_G",
        K3G = 0x17 => "K3_G",
        K0B = 0x18 => "K0_B",
        K1B = 0x19 => "K1_B",
        K2B = 0x1A => "K2_B",
        K3B = 0x1B => "K3_B",
        K0A = 0x1C => "K0_A",
        K1A = 0x1D => "K1_A",
        K2A = 0x1E => "K2_A",
        K3A = 0x1F => "K3_A",
    }
}

gx_enum! {
    /// GXTexCoordID (GXEnum.h:66–75)
    TexCoordId {
        Texcoord0 = 0x00 => "TEXCOORD0",
        Texcoord1 = 0x01 => "TEXCOORD1",
        Texcoord2 = 0x02 => "TEXCOORD2",
        Texcoord3 = 0x03 => "TEXCOORD3",
        Texcoord4 = 0x04 => "TEXCOORD4",
        Texcoord5 = 0x05 => "TEXCOORD5",
        Texcoord6 = 0x06 => "TEXCOORD6",
        Texcoord7 = 0x07 => "TEXCOORD7",
        Null = 0xFF => "TEXCOORD_NULL",
    }
}

gx_enum! {
    /// GXTexMapID (GXEnum.h:32–41)
    TexMapId {
        Texmap0 = 0x00 => "TEXMAP0",
        Texmap1 = 0x01 => "TEXMAP1",
        Texmap2 = 0x02 => "TEXMAP2",
        Texmap3 = 0x03 => "TEXMAP3",
        Texmap4 = 0x04 => "TEXMAP4",
        Texmap5 = 0x05 => "TEXMAP5",
        Texmap6 = 0x06 => "TEXMAP6",
        Texmap7 = 0x07 => "TEXMAP7",
        Null = 0xFF => "TEXMAP_NULL",
    }
}

gx_enum! {
    /// GXChannelID (GXEnum.h:83–88)
    ColorChannelId {
        Color0 = 0x00 => "COLOR0",
        Color1 = 0x01 => "COLOR1",
        Alpha0 = 0x02 => "ALPHA0",
        Alpha1 = 0x03 => "ALPHA1",
        Color0A0 = 0x04 => "COLOR0A0",
        Color1A1 = 0x05 => "COLOR1A1",
        ColorZero = 0x06 => "COLOR_ZERO",
        AlphaBump = 0x07 => "ALPHA_BUMP",
        AlphaBumpN = 0x08 => "ALPHA_BUMP_N",
        Null = 0xFF => "COLOR_NULL",
    }
}

gx_enum! {
    /// GXFogType (GXEnum.h:485–495); J3D stores the perspective variants
    FogType {
        Off = 0x0 => "OFF",
        Linear = 0x2 => "LINEAR",
        Exp = 0x4 => "EXP",
        Exp2 = 0x5 => "EXP2",
        RevExp = 0x6 => "REVEXP",
        RevExp2 = 0x7 => "REVEXP2",
    }
}

gx_enum! {
    /// J3D TexMatrix projection mode (gclib TexMtxProjection)
    TexMtxProjection {
        Mtx3x4 = 0x0 => "MTX3x4",
        Mtx2x4 = 0x1 => "MTX2x4",
    }
}

gx_enum! {
    /// J3D TexMatrix map mode (gclib TexMtxMapMode)
    TexMtxMapMode {
        None = 0x00 => "None_",
        EnvmapBasic = 0x01 => "EnvmapBasic",
        ProjmapBasic = 0x02 => "ProjmapBasic",
        ViewProjmapBasic = 0x03 => "ViewProjmapBasic",
        Unknown04 = 0x04 => "UNKNOWN_04",
        Unknown05 = 0x05 => "UNKNOWN_05",
        EnvmapOld = 0x06 => "EnvmapOld",
        Envmap = 0x07 => "Envmap",
        Projmap = 0x08 => "Projmap",
        ViewProjmap = 0x09 => "ViewProjmap",
        EnvmapOldEffectMtx = 0x0A => "EnvmapOldEffectMtx",
        EnvmapEffectMtx = 0x0B => "EnvmapEffectMtx",
    }
}

gx_enum! {
    /// GXAttr (GXEnum.h:199–226): vertex attribute id, in a VTX1 format entry
    /// or a SHP1 vertex-descriptor entry. cl.bdl uses only PNMTXIDX/POS/NRM/TEX0,
    /// but the full set is recognized so off-spec attrs fail loudly.
    Attr {
        Pnmtxidx = 0x00 => "PNMTXIDX",
        Tex0Mtxidx = 0x01 => "TEX0MTXIDX",
        Tex1Mtxidx = 0x02 => "TEX1MTXIDX",
        Tex2Mtxidx = 0x03 => "TEX2MTXIDX",
        Tex3Mtxidx = 0x04 => "TEX3MTXIDX",
        Tex4Mtxidx = 0x05 => "TEX4MTXIDX",
        Tex5Mtxidx = 0x06 => "TEX5MTXIDX",
        Tex6Mtxidx = 0x07 => "TEX6MTXIDX",
        Tex7Mtxidx = 0x08 => "TEX7MTXIDX",
        Pos = 0x09 => "POS",
        Nrm = 0x0A => "NRM",
        Clr0 = 0x0B => "CLR0",
        Clr1 = 0x0C => "CLR1",
        Tex0 = 0x0D => "TEX0",
        Tex1 = 0x0E => "TEX1",
        Tex2 = 0x0F => "TEX2",
        Tex3 = 0x10 => "TEX3",
        Tex4 = 0x11 => "TEX4",
        Tex5 = 0x12 => "TEX5",
        Tex6 = 0x13 => "TEX6",
        Tex7 = 0x14 => "TEX7",
        Nbt = 0x19 => "NBT",
        Null = 0xFF => "NULL",
    }
}

gx_enum! {
    /// GXAttrType (GXEnum.h:265–268): how a SHP1 display-list attribute is
    /// encoded on the wire. cl.bdl reads every array attr as INDEX16.
    AttrInputType {
        None = 0x0 => "NONE",
        Direct = 0x1 => "DIRECT",
        Index8 = 0x2 => "INDEX8",
        Index16 = 0x3 => "INDEX16",
    }
}

gx_enum! {
    /// GXCompType (GXEnum.h): component storage type for POS/NRM/TEX arrays.
    /// (The color variants share these byte values but cl.bdl has no color
    /// arrays.) Fixed-point integer components divide by 2^shift.
    ComponentType {
        U8 = 0x0 => "U8",
        S8 = 0x1 => "S8",
        U16 = 0x2 => "U16",
        S16 = 0x3 => "S16",
        F32 = 0x4 => "F32",
    }
}

gx_enum! {
    /// GXPrimitive (GXEnum.h:7–13): the top 5 bits of a display-list opcode
    /// (low 3 bits are the VAT index, 0 in cl.bdl). 0x00 is a NOP/pad byte,
    /// handled separately. cl.bdl is triangle strips only.
    PrimitiveType {
        Quads = 0x80 => "QUADS",
        Triangles = 0x90 => "TRIANGLES",
        TriangleStrip = 0x98 => "TRIANGLESTRIP",
        TriangleFan = 0xA0 => "TRIANGLEFAN",
        Lines = 0xA8 => "LINES",
        LineStrip = 0xB0 => "LINESTRIP",
        Points = 0xB8 => "POINTS",
    }
}

gx_enum! {
    /// SHP1 J3DShapeInitData.mShapeMtxType (J3DShapeFactory.h): how a shape's
    /// vertices reference draw matrices. cl.bdl uses Single (rigid overlays)
    /// and Multi (weighted body parts); billboards are hard-errored downstream.
    ShapeMatrixType {
        Single = 0x0 => "Single",
        Billboard = 0x1 => "Billboard",
        BillboardY = 0x2 => "BillboardY",
        Multi = 0x3 => "Multi",
    }
}

gx_enum! {
    /// INF1 hierarchy node type (J3DModelLoader / J3DModelData.cpp
    /// makeHierarchy): the scene-graph stream defining joint parentage and
    /// draw order.
    InfNodeType {
        Finish = 0x00 => "FINISH",
        OpenChild = 0x01 => "OPEN",
        CloseChild = 0x02 => "CLOSE",
        Joint = 0x10 => "JOINT",
        Material = 0x11 => "MATERIAL",
        Shape = 0x12 => "SHAPE",
    }
}

gx_enum! {
    /// INF1 load-flags low nibble (J3DModelLoader::readInformation): the joint
    /// matrix-calc / rotation-composition rule. cl.bdl is MAYA.
    MatrixScalingRule {
        Basic = 0x0 => "BASIC",
        Softimage = 0x1 => "SOFTIMAGE",
        Maya = 0x2 => "MAYA",
    }
}

/// Reads a bool byte, rejecting anything but 0/1 (junk would otherwise
/// silently become `true`).
pub fn gx_bool(value: u8, kind: &'static str) -> Result<bool, GxEnumError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(GxEnumError {
            kind,
            value: value as u32,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values_map() {
        assert_eq!(ImageFormat::try_from(0xE), Ok(ImageFormat::Cmpr));
        assert_eq!(ImageFormat::try_from(0x8), Ok(ImageFormat::C4));
        assert_eq!(CullMode::try_from(2), Ok(CullMode::Back));
        assert_eq!(TexGenType::try_from(0xA), Ok(TexGenType::Srtg));
        assert_eq!(TexGenMatrix::try_from(60), Ok(TexGenMatrix::Identity));
        assert_eq!(CombineColor::try_from(0xE), Ok(CombineColor::Konst));
        assert_eq!(KonstColorSel::try_from(0x0C), Ok(KonstColorSel::K0));
        assert_eq!(ColorChannelId::try_from(0xFF), Ok(ColorChannelId::Null));
    }

    #[test]
    fn gaps_are_errors_with_kind() {
        assert_eq!(
            ImageFormat::try_from(0x7),
            Err(GxEnumError {
                kind: "ImageFormat",
                value: 0x7
            })
        );
        assert_eq!(
            KonstColorSel::try_from(0x08),
            Err(GxEnumError {
                kind: "KonstColorSel",
                value: 0x08
            })
        );
        assert_eq!(
            TevOp::try_from(0x2),
            Err(GxEnumError {
                kind: "TevOp",
                value: 0x2
            })
        );
        assert_eq!(
            FogType::try_from(0x1),
            Err(GxEnumError {
                kind: "FogType",
                value: 0x1
            })
        );
    }

    #[test]
    fn canonical_spellings_match_oracle_vocabulary() {
        // spot checks against gclib's enum member names (the shared spec)
        assert_eq!(CullMode::Back.to_string(), "Cull_Back");
        assert_eq!(CompareType::LessEqual.to_string(), "Less_Equal");
        assert_eq!(
            BlendFactor::InverseSourceAlpha.to_string(),
            "Inverse_Source_Alpha"
        );
        assert_eq!(KonstAlphaSel::K0A.to_string(), "K0_A");
        assert_eq!(TevScale::Scale1.to_string(), "SCALE_1");
        assert_eq!(DiffuseFunction::None.to_string(), "None_");
    }

    #[test]
    fn geometry_enums() {
        assert_eq!(Attr::try_from(0x09), Ok(Attr::Pos));
        assert_eq!(Attr::try_from(0xFF), Ok(Attr::Null));
        assert_eq!(AttrInputType::try_from(3), Ok(AttrInputType::Index16));
        assert_eq!(ComponentType::try_from(4), Ok(ComponentType::F32));
        assert_eq!(
            PrimitiveType::try_from(0x98),
            Ok(PrimitiveType::TriangleStrip)
        );
        assert_eq!(ShapeMatrixType::try_from(3), Ok(ShapeMatrixType::Multi));
        assert_eq!(InfNodeType::try_from(0x12), Ok(InfNodeType::Shape));
        assert_eq!(MatrixScalingRule::try_from(2), Ok(MatrixScalingRule::Maya));
        // canonical spellings (shared with the geometry oracle)
        assert_eq!(PrimitiveType::TriangleStrip.to_string(), "TRIANGLESTRIP");
        assert_eq!(ShapeMatrixType::Multi.to_string(), "Multi");
        assert_eq!(InfNodeType::OpenChild.to_string(), "OPEN");
    }

    #[test]
    fn geometry_enum_gaps() {
        // 0x0F..0x18 (between TEX3=0x10? no) — pick a genuine gap: Attr 0x15
        assert_eq!(
            Attr::try_from(0x15),
            Err(GxEnumError {
                kind: "Attr",
                value: 0x15
            })
        );
        assert_eq!(
            PrimitiveType::try_from(0x00),
            Err(GxEnumError {
                kind: "PrimitiveType",
                value: 0x00
            })
        );
        assert_eq!(
            InfNodeType::try_from(0x03),
            Err(GxEnumError {
                kind: "InfNodeType",
                value: 0x03
            })
        );
    }

    #[test]
    fn strict_bools() {
        assert_eq!(gx_bool(0, "test"), Ok(false));
        assert_eq!(gx_bool(1, "test"), Ok(true));
        assert_eq!(
            gx_bool(2, "test"),
            Err(GxEnumError {
                kind: "test",
                value: 2
            })
        );
    }
}
