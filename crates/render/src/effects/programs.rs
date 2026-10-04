//! Camera-distance sizing is compiled into individual eft vertex programs.
//! Nonzero static-block defaults at 0x740/0x744 do not enable it. These
//! program families are recovered from the captured programs' uniform reads
//! and formulas (docs/research/eft-shaders.md §5.7).

/// 0: no sizing, 1: near shrink, 2: far expansion, 3: both.
pub fn distance_scale(vertex: &str) -> u32 {
    match vertex.get(..12) {
        Some("04eb9ff76a55") | Some("0dd1bfb74d50") | Some("12f8cd23f6b6")
        | Some("1437ee24dbb7") | Some("161f26c67e11") | Some("1876dfb31502")
        | Some("194b20e895b6") | Some("197290c72a3f") | Some("1aefa795cf7a")
        | Some("1b73a1c9bfda") | Some("1f6f02a08d37") | Some("22bc60a0c296")
        | Some("2860496eb8da") | Some("2afdbe547b10") | Some("2f0dda43dd18")
        | Some("2f5ddaf73d2e") | Some("2fb32a574ec4") | Some("33b7962cdd8e")
        | Some("34607fd6f028") | Some("370b7c969159") | Some("39a6c7725776")
        | Some("3a044a86f93f") | Some("3b9bd02e9b69") | Some("40a62fdbc338")
        | Some("4906b47ab3a4") | Some("4c4255090667") | Some("4d0190538c00")
        | Some("4d07292f9098") | Some("4db0dbeda343") | Some("4e43a58b4123")
        | Some("4fe10550d77d") | Some("53c6d0ca088c") | Some("571daa00e3a8")
        | Some("584d94cc7241") | Some("5a41a0be8165") | Some("5ef5da1019b6")
        | Some("608b893159f4") | Some("6681f1af414d") | Some("6b9646dfc921")
        | Some("6f7dd45db096") | Some("72e438259ad1") | Some("74d9e70c96a9")
        | Some("7759bc085213") | Some("7cb76c766c6c") | Some("7dfa76f6925a")
        | Some("82e0e7c92bf8") | Some("852e3986be1e") | Some("8e99b086c1bd")
        | Some("8f65ca179077") | Some("8faa26a4c47b") | Some("91a160137c24")
        | Some("92a365774cf9") | Some("96358409957f") | Some("99c7dc91d639")
        | Some("9b842f32aea7") | Some("9c1da57d5936") | Some("9c4b8f8547fe")
        | Some("9d3465e31603") | Some("9d42ac55412b") | Some("9f1af33b2626")
        | Some("a30310148c15") | Some("a493d9d31ae4") | Some("a6294d6752fb")
        | Some("a7df97fab1ae") | Some("aac37e8d3f05") | Some("ac90c162d0f0")
        | Some("ae60dee92029") | Some("b30fa1cfed7b") | Some("b8c83672a901")
        | Some("be18eabdb5aa") | Some("bf07301c52dd") | Some("c1778a75192b")
        | Some("c195faf59a0b") | Some("c286bf32cbcf") | Some("c2ea4ac55f29")
        | Some("c394f47d6e1f") | Some("c57cc8d391a4") | Some("c5ef9499da5c")
        | Some("c69576b63667") | Some("ca0b6dff147b") | Some("cb0c37feec30")
        | Some("cbbc19ac5b12") | Some("cbfb6fe7d545") | Some("cce1b660bb7b")
        | Some("ce77fd289ea6") | Some("d306e3832734") | Some("d6e5fd7e0554")
        | Some("e0a9f75d496f") | Some("e7edfdb92521") | Some("ef48678b50fa")
        | Some("f0022e007828") | Some("f15f4309776d") | Some("f47e79bc7f5d")
        | Some("f81f2f022009") | Some("fb7d94aaaa63") | Some("fcc517fce51b")
        | Some("fd900cc2d6b3") | Some("fdfeeebb34f2") | Some("fec38a97f6fd")
        | Some("ff8563c5aab9") => 1,
        Some("36a0d3baca4d") | Some("b3af543f6578") | Some("c0b91eb1f053")
        | Some("ce4c605021cd") | Some("ebee64f032d0") => 2,
        Some("0774fcabb821") | Some("123be76ef9fe") | Some("3c28c02039aa")
        | Some("54e8848934da") | Some("6a878ec00ca3") | Some("82c87f1471f5")
        | Some("dab04af5345b") | Some("dc4ebbda65f9") | Some("e473e0260a86")
        | Some("eddcc33473d2") | Some("fba60c9213a1") => 3,
        // SI-EFX-35: programs without a captured sizing formula keep world size.
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haze_and_volcano_programs_do_not_expand_with_distance() {
        for program in [
            "99aaf84627f7a3b0fe0e63cb20cc3b411d8cf529", // LavaHaze
            "e6cc513b8fefe545503cd4f5bcdcc6b044ee156f", // MountainCloud
            "a3643af1cf69ffa91cd4a04d565adc3abce31f63", // Cloud_Radial
            "b41c13293c1cc9ef6347a7cf8ba21d745f99379b", // Xlu_Plume_1
        ] {
            assert_eq!(distance_scale(program), 0, "{program}");
        }
    }

    #[test]
    fn sizing_preserves_each_programs_near_and_far_modes() {
        assert_eq!(distance_scale("e0a9f75d496f"), 1); // Rain: shrink only.
        assert_eq!(distance_scale("36a0d3baca4d"), 2); // Expansion only.
        assert_eq!(distance_scale("123be76ef9fe"), 3);
        assert_eq!(distance_scale("0774fcabb821"), 3);
        assert_eq!(distance_scale(""), 0);
        assert_eq!(distance_scale("unknown program"), 0);
    }
}
