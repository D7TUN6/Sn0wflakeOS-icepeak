use strict;
use warnings;

my $path = shift @ARGV or die "usage: ppmbbox.pl file.ppm\n";
open(my $fh, "<", $path) or die "$path: $!";
binmode $fh;
my $magic = <$fh>;
my $line = <$fh>;
$line = <$fh> if $line =~ /^\s*#/;
my ($w, $h) = split(/\s+/, $line);
my $max = <$fh>;
my $data = "";
read($fh, $data, $w * $h * 3);
my ($minx, $miny, $maxx, $maxy) = ($w, $h, -1, -1);
for my $y (0 .. $h - 1) {
    for my $x (0 .. $w - 1) {
        my ($r, $g, $b) = unpack("C3", substr($data, ($y * $w + $x) * 3, 3));
        if ($r || $g || $b) {
            $minx = $x if $x < $minx;
            $maxx = $x if $x > $maxx;
            $miny = $y if $y < $miny;
            $maxy = $y if $y > $maxy;
        }
    }
}
print "bbox x=$minx..$maxx y=$miny..$maxy\n";
my $ch = 16;
my $cw = 8;
for (my $cy = 0; $cy + $ch <= $h; $cy += $ch) {
    my $row = "";
    for (my $cx = 0; $cx + $cw <= $w; $cx += $cw) {
        my $n = 0;
        for my $y (0 .. $ch - 1) {
            for my $x (0 .. $cw - 1) {
                my $off = (($cy + $y) * $w + ($cx + $x)) * 3;
                my ($r, $g, $b) = unpack("C3", substr($data, $off, 3));
                $n++ if $r || $g || $b;
            }
        }
        $row .= $n == 0 ? " " : ($n < 5 ? "." : ($n < 15 ? "+" : "#"));
    }
    $row =~ s/\s+$//;
    printf "%2d|%s\n", $cy / $ch, $row if length($row);
}
