use strict;
use warnings;

my $path = shift @ARGV or die "usage: ppmshow.pl file.ppm\n";
open(my $fh, "<", $path) or die "$path: $!";
binmode $fh;
my $magic = <$fh>;
die "not P6" unless defined $magic && $magic =~ /^P6/;
my $line = <$fh>;
$line = <$fh> if $line =~ /^\s*#/;
my ($w, $h) = split(/\s+/, $line);
my $max = <$fh>;
die "bad dims" unless $w && $h;
my $data = "";
read($fh, $data, $w * $h * 3);
die "short data" unless length($data) == $w * $h * 3;

my $lit = 0;
for my $i (0 .. $w * $h - 1) {
    my ($r, $g, $b) = unpack("C3", substr($data, $i * 3, 3));
    $lit++ if $r || $g || $b;
}
print "size ${w}x${h} nonblack=$lit\n";

my $cw = 8;
my $ch = 16;
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
        $row .= $n ? "#" : ".";
    }
    $row =~ s/\.+$//;
    print "$row\n" if length($row);
}
