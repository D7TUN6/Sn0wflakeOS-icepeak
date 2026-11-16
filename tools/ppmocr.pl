use strict;
use warnings;

my ($ppm, $fontfile) = @ARGV;
$fontfile ||= "kernel/src/font.rs";

open(my $ff, "<", $fontfile) or die "$fontfile: $!";
my @font;
while (my $l = <$ff>) {
    while ($l =~ /0x([0-9a-fA-F]{2})/g) {
        push @font, hex($1);
    }
}
die "font has " . scalar(@font) . " bytes" unless @font == 4096;

open(my $fh, "<", $ppm) or die "$ppm: $!";
binmode $fh;
my $magic = <$fh>;
my $line = <$fh>;
my ($w, $h) = split(/\s+/, $line);
my $max = <$fh>;
my $data = "";
read($fh, $data, $w * $h * 3);

my $cols = int($w / 8);
my $rows = int($h / 16);
for my $cy (0 .. $rows - 1) {
    my $out = "";
    for my $cx (0 .. $cols - 1) {
        my @cell;
        for my $y (0 .. 15) {
            my $bits = 0;
            for my $x (0 .. 7) {
                my ($r, $g, $b) = unpack("C3", substr($data, ((($cy * 16 + $y) * $w) + ($cx * 8 + $x)) * 3, 3));
                $bits = ($bits << 1) | (($r || $g || $b) ? 1 : 0);
            }
            push @cell, $bits;
        }
        my $best = -1;
        my $bestd = 999;
        for my $g (0 .. 255) {
            my $d = 0;
            for my $y (0 .. 15) {
                my $v = $font[$g * 16 + $y] ^ $cell[$y];
                $d += ($v & 1) + (($v >> 1) & 1) + (($v >> 2) & 1) + (($v >> 3) & 1)
                    + (($v >> 4) & 1) + (($v >> 5) & 1) + (($v >> 6) & 1) + (($v >> 7) & 1);
            }
            if ($d < $bestd) { $bestd = $d; $best = $g; }
            last if $d == 0;
        }
        $out .= $bestd == 0 ? chr($best) : ($bestd <= 2 ? lc(chr($best)) : "?");
    }
    $out =~ s/\s+$//;
    printf "%2d|%s\n", $cy, $out if length($out);
}
