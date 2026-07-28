difference() {
    translate([-4/2, -76.2/2, 0]) cube([4,76.2,2.5]);
    translate([-6/2, -65/2, -1.25]) cube([6,65,2.5]);
}
translate([0,-76.2/2,0]) cylinder(5, 0.9, 0.9, $fn=8);
translate([0,76.2/2,0]) cylinder(5, 0.9, 0.9, $fn=8);
translate([-4/2,-10.0]) cube([4,20,2]);