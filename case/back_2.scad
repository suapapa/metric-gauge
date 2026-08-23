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
        
        translate([-25,15,-20]) cylinder(30, 15/2, 15/2);
        translate([-25-2.1,15-(16/2),-20]) cube([4.2,16,30]);
        
        // #translate([0,-30,30]) cube([20,50,20], center=true);
    }
    apillas(hw, hh);
}

module abox(w, h, r=2){
    translate([-w/2, -h/2, 0]) difference() {
        hull() {
            translate([0, 0, -20]) cylinder(70, r, r, $fn=18);
            translate([w, 0, -20]) cylinder(70, r, r, $fn=18);
            translate([0, h, -30]) rotate([-a,0,0])cylinder(90, r, r, $fn=18);
            translate([w, h, -30]) rotate([-a,0,0]) cylinder(90, r, r, $fn=18);
        }
        translate([-200,-200,-410]) cube(400);
        translate([-200,-200,90]) rotate([-a,0,0]) cube(400);
    }
}

module apillas(w, h, r=5){
    translate([-w/2, -h/2, 0]) difference() {
        union() {
            rotate([-a,0,0]) cylinder(50, r, r, $fn=8);
            translate([w, 0, 0]) rotate([-a,0,0]) cylinder(50, r, r, $fn=8);
            translate([0, h, -10]) rotate([-a,0,0])cylinder(70, r, r, $fn=8);
            translate([w, h, -10]) rotate([-a,0,0]) cylinder(70, r, r, $fn=8);
        }
        hr=1.5;
        rotate([-a, 0, 0]) translate([0,0,1]) cylinder(50, hr, hr, $fn=6);
        translate([w, 0, 0]) rotate([-a,0,0]) translate([0,0,1]) cylinder(50, hr, hr, $fn=6);
        translate([0, h, -10]) rotate([-a,0,0]) translate([0,0,1]) cylinder(70, hr, hr, $fn=6);
        translate([w, h, -10]) rotate([-a,0,0]) translate([0,0,1]) cylinder(70, hr, hr, $fn=6);
        translate([-200,-200,-400]) cube(400);
        translate([-200,-200,90-6.8-2.5]) rotate([-a,0,0]) cube(400);
    }
}

module cb(w, h) {
    translate([-w/2,-h/2,-20]) cube([w, h, 100]);
}