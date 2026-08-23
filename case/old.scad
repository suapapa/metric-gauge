
difference(){
    union(){
        difference() {
            abox(108,62);
            translate([0,0,1.5]) abox(108-3,62-3);
            
            //translate([0,15,4]) abox(108-5-5,62-10-25,d=0);
        }
        pillas(95,52,r=5.5,d=0);
    }
    pillas(95,52,r=1.5,d=1);
    
    #translate([50,-62/2-1.5+11.8,35]) cube([100,11.2+0.2,12.4+0.2]);
}


module abox(w, h, r=3){
    w = w-r*2;
    h = h-r*2;
    t=80;
    translate([-w/2, -h/2, 0]) difference() {
        hull() {
            cylinder(t, r, r, $fn=18);
            translate([w, 0, 0]) cylinder(t, r, r, $fn=18);
            translate([0, h, 0]) cylinder(t, r, r, $fn=18);
            translate([w, h, 0]) cylinder(t, r, r, $fn=18);
        }
    }
}

module pillas(w, h, r=3, d=0) {
    a=0;
    t=5;
    translate([-w/2, -h/2, 80-5+d]) difference(){    
        union(){
            cylinder(t, r, r, $fn=6);
            translate([w, 0, 0]) cylinder(t, r, r, $fn=6);
            translate([0, h, 0]) cylinder(t, r, r, $fn=6);
            translate([w, h, 0]) cylinder(t, r, r, $fn=6);
        }
    }

}