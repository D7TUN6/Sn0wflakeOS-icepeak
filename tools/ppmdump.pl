use strict;
use warnings;
my $path = shift @ARGV or die;
open(my $fh, "<", $path) or die "$path: $!";
binmode $fh;
my $magic = <$fh>;
my $line = <$fh>;
my ($w, $h) = split(/\s+/, $line);
my $max = <$fh>;
my $data = "";
read($fh, $data, $w * $h * 3);
my ($x0, $y0, $x1, $y1) = @ARGV;
($x0, $y0, $x1, $y1) = (0, 0, 128, 48) unless defined $x1;
for my $y ($y0 .. $y1 - 1) {
    my $row = "";
    for my $x ($x0 .. $x1 - 1) {
        my ($r, $g, $b) = unpack("C3", substr($data, ($y * $w + $x) * 3, 3));
        $row .= ($r || $g || $b) ? "#" : " ";
    }
    $row =~ s/\s+$//;
    print "$row\n";
}
