//! Typed enums for every GX byte the converter reads. Parse-don't-validate:
//! every raw value must map to a known variant or the parse fails with the
//! field name and value. Numeric values verified against
//! ../tww/include/dolphin/gx/GXEnum.h; canonical `Display` spellings are the
//! shared vocabulary of the MAT3 diff gate (scripts/link_mat3_table.py prints
//! the same names from gclib's enums).
//!
//! The enums the manifest serializes (`CullMode`, `CompareType`, the blend and
//! channel state, texture wrap/filter, …) are defined in the library's
//! `model_manifest` alongside the schema that carries them, and re-exported here
//! so every `crate::gx::types::` path names the same type either way. The **TEV
//! and texgen vocabulary** lives there too, even though the manifest carries it
//! as raw `u8`: `tev_pack` re-checks those bytes on the way to the GPU and
//! prints the stage equations, so the library needs the same `TryFrom` and the
//! same canonical `Display` spellings. What stays below is everything only the
//! converter parses: the BMD/BTI vocabulary.

use vulkan_slang_renderer::gx_enum;

pub use vulkan_slang_renderer::gx::model_manifest::{
    AlphaOp, AttenuationFunction, BlendFactor, BlendMode, ColorChannelId, ColorSrc, CombineAlpha,
    CombineColor, CompareType, CullMode, DiffuseFunction, FilterMode, GxEnumError, KonstAlphaSel,
    KonstColorSel, LogicOp, PixelEngineMode, Register, TevBias, TevOp, TevScale, TexCoordId,
    TexGenMatrix, TexGenSrc, TexGenType, TexMapId, WrapMode,
};

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
