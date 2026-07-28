z=1.5;
pw=78.74;
ph=41.91;
plw=89.74;
plh=63;
g=(plw-pw)/2; // 5.5

difference() {
    union() {
        translate([g,g,0]) pillas(pw,ph,5.2,3);
        plate(plw,plh,2);
    }
    translate([g,g,-2]) pillas(pw,ph,10,1.7);
    translate([0,g,0]) lcdholes();
}

module pillas(w, h, z, r) {
    w=78.74;
    h=41.91;
    translate([0,0,0]) union() {
        cylinder(z,r,r,$fn=20);
        translate([w,0,0]) cylinder(z,r,r,$fn=20);
        translate([0,h,0]) cylinder(z,r,r,$fn=20);
        translate([w,h,0]) cylinder(z,r,r,$fn=20);
    }
}

module plate(w, h, r) {
    z=1.5;
    linear_extrude(z) hull() {
        translate([r,r,0]) circle(r,$fn=20);
        translate([r,h-r,0]) circle(r,$fn=20);
        translate([w-r,r,0]) circle(r,$fn=20);
        translate([w-r,h-r,0]) circle(r,$fn=20);        
    }
}

module lcdholes() {
    h=20.955;
    w=38.1;
    translate([(plw-w)/2,h,-0.4]) cylinder(2, 35/2, 32.4/2,$fn=120);
    translate([(plw-w)/2+w,h,-0.4]) cylinder(2, 35/2, 32.4/2,$fn=120);
}