use strict;
use warnings;

my $path = "/nix/store/jk66bz3lrb948zi24hhyfy8wqvki1na3-kbd-2.9.0/share/consolefonts/default8x16.psfu.gz";
open(my $fh, "-|", "gzip", "-dc", $path) or die "gzip: $!";
binmode $fh;
read($fh, my $hdr, 32) == 32 or die "short header";
my ($magic, $ver, $hdrsize, $flags, $length, $charsize, $height, $width) = unpack("V8", $hdr);
die sprintf("bad magic %08x", $magic) unless $magic == 0x864ab572;
print STDERR "magic=$magic version=$ver hdrsize=$hdrsize flags=$flags length=$length charsize=$charsize height=$height width=$width\n";
die "not 8x16" unless $width == 8 && $height == 16;
# The 32-byte header has already been consumed, so skip only the remaining
# bytes up to `hdrsize` before reading glyph data.
read($fh, my $skip, $hdrsize - 32) if $hdrsize > 32;
my $glyphs = "";
while (length($glyphs) < 256 * $charsize) {
    my $n = read($fh, $glyphs, 256 * $charsize - length($glyphs), length($glyphs));
    last unless $n;
}
open(my $out, ">", "/tmp/font.rs") or die;
print $out "// 8x16 console font, extracted from the public kbd 2.9.0 default8x16.psfu.\n";
print $out "// 256 glyphs, one byte per scanline, MSB is the leftmost pixel.\n\n";
print $out "pub static FONT8X16: [u8; 4096] = [\n";
for my $i (0..4095) {
    my $b = ord(substr($glyphs, $i, 1));
    print $out sprintf("0x%02x,", $b);
    print $out (($i % 16 == 15) ? "\n" : " ");
}
print $out "];\n";
close $out;
