a=15;

hw=78.74;
hh=41.91;

w=hw+4; // 100;
h=hh+4; // 60;

union() {
    difference() {
        abox(w+3, h+3);
        translate([0,0,1.5]) abox(w, h);
        
        translate([0,-15,0]) cb(14.2, 3.2);
        translate([0,-15,0]) cb(13, 5.7);
    }
    apillas(hw, hh);
}

module abox(w, h, r=2){
    translate([-w/2, -h/2, 0]) difference() {
        hull() {
            cylinder(50, r, r, $fn=18);
            translate([w, 0, 0]) cylinder(50, r, r, $fn=18);
            translate([0, h, -10]) rotate([-a,0,0])cylinder(70, r, r, $fn=18);
            translate([w, h, -10]) rotate([-a,0,0]) cylinder(70, r, r, $fn=18);
        }
        translate([-200,-200,-400]) cube(400);
        translate([-200,-200,90]) rotate([-a,0,0]) cube(400);
    }
}

module apillas(w, h, r=5){
    translate([-w/2, -h/2, 0]) difference() {
        union() {
            rotate([-a,0,0]) cylinder(50, r, r, $fn=6);
            translate([w, 0, 0]) rotate([-a,0,0]) cylinder(50, r, r, $fn=6);
            translate([0, h, -10]) rotate([-a,0,0])cylinder(70, r, r, $fn=6);
            translate([w, h, -10]) rotate([-a,0,0]) cylinder(70, r, r, $fn=6);
        }
        hr=1.5;
        rotate([-a, 0, 0]) translate([0,0,1]) cylinder(50, hr, hr, $fn=6);
        translate([w, 0, 0]) rotate([-a,0,0]) translate([0,0,1]) cylinder(50, hr, hr, $fn=6);
        translate([0, h, -10]) rotate([-a,0,0]) translate([0,0,1]) cylinder(70, hr, hr, $fn=6);
        translate([w, h, -10]) rotate([-a,0,0]) translate([0,0,1]) cylinder(70, hr, hr, $fn=6);
        translate([-200,-200,-400]) cube(400);
        translate([-200,-200,90-6.8]) rotate([-a,0,0]) cube(400);
    }
}

module cb(w, h) {
    translate([-w/2,-h/2,-20]) cube([w, h, 100]);
}