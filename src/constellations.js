// Constellation geometry — extracted verbatim from Help Lab v2.dc.html
//
// pts:   [x, y] in a local px space (roughly 0-90). The SVG viewBox for each
//        constellation is sized from the point extents + 8 on each axis.
// lines: [a, b] index pairs into pts — the segments drawn between stars.
// pos:   [x px, y %] the AUTHORED position. At runtime x is discarded and
//        recomputed per drift band (see night-scene.md section 2); y is kept
//        and jittered by +/-5.5, clamped to 3-70.
// Star radius: index 0 renders at r 1.9, all others at r 1.4 (+0.5 when lit).

export const CONSTELLATIONS = [
    { name: "Orion", season: "Winter", fact: "The Hunter. Three belt stars in a perfect row; red Betelgeuse marks a shoulder, blue-white Rigel a foot.", pos: [96, 44.2], pts: [[10,4],[54,10],[22,38],[32,44],[42,50],[6,84],[62,78]], lines: [[0,2],[1,4],[2,3],[3,4],[2,5],[4,6]] },
    { name: "Scorpius", season: "Summer", fact: "The scorpion that felled Orion — a long J-shaped hook low on the horizon, with red Antares as its heart.", pos: [263, 9.4], pts: [[70,4],[60,14],[52,26],[50,42],[54,58],[64,68],[78,72],[88,64]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6],[6,7]] },
    { name: "Crux", season: "Autumn", fact: "The Southern Cross — the smallest constellation, and the one southern sailors steer by to find the pole.", pos: [418, 66.8], pts: [[34,2],[30,34],[26,66],[6,36],[56,30]], lines: [[0,1],[1,2],[3,1],[1,4]] },
    { name: "Sagittarius", season: "Summer", fact: "The Archer, though its bright stars draw a teapot — spout, lid and handle, pouring into the Milky Way.", pos: [147, 24.6], pts: [[32,2],[16,22],[50,18],[56,42],[22,46],[4,32],[70,26],[66,44]], lines: [[0,1],[0,2],[1,2],[2,3],[3,4],[4,1],[1,5],[5,4],[3,6],[6,7],[7,3]] },
    { name: "Ursa Major", season: "Spring", fact: "The Great Bear. Its seven brightest stars are the Big Dipper — the two front bowl stars point straight to Polaris.", pos: [560, 12.1], pts: [[2,38],[20,30],[38,32],[54,40],[76,34],[86,54],[62,60]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6],[6,3]] },
    { name: "Cassiopeia", season: "Autumn", fact: "The vain queen on her throne — an unmistakable W of five stars, circling the pole opposite the Big Dipper.", pos: [724, 38.4], pts: [[2,30],[20,8],[40,26],[60,4],[78,32]], lines: [[0,1],[1,2],[2,3],[3,4]] },
    { name: "Cygnus", season: "Summer", fact: "The Swan, flying down the Milky Way. Its shape earns it the name Northern Cross; bright Deneb marks the tail.", pos: [332, 51.3], pts: [[44,2],[44,28],[44,54],[44,80],[12,40],[76,36]], lines: [[0,1],[1,2],[2,3],[4,1],[1,5]] },
    { name: "Leo", season: "Spring", fact: "The Lion. A backwards question mark — the Sickle — forms its mane, with Regulus at the heart.", pos: [1186, 15.7], pts: [[70,10],[58,4],[46,10],[44,24],[54,34],[74,36],[24,58],[6,64],[30,72]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,8],[8,7],[7,6],[6,5]] },
    { name: "Taurus", season: "Winter", fact: "The Bull. A V of stars — the Hyades cluster — draws its face; orange Aldebaran is the bull's fiery eye.", pos: [1042, 59.7], pts: [[10,8],[26,30],[36,44],[48,34],[68,10]], lines: [[0,1],[1,2],[2,3],[3,4]] },
    { name: "Gemini", season: "Winter", fact: "The Twins. Castor and Pollux head two parallel chains of stars, side by side like stick figures.", pos: [22, 68.5], pts: [[14,6],[10,26],[8,48],[6,68],[40,4],[44,24],[48,46],[52,66]], lines: [[0,1],[1,2],[2,3],[4,5],[5,6],[6,7],[0,4]] },
    { name: "Canis Major", season: "Winter", fact: "The Great Dog at Orion's heel — home of Sirius, the brightest star in Earth's night sky.", pos: [906, 8.2], pts: [[46,6],[38,20],[46,34],[30,44],[52,48],[40,62],[28,74],[50,74]], lines: [[0,1],[1,2],[2,3],[2,4],[4,5],[5,6],[5,7]] },
    { name: "Pegasus", season: "Autumn", fact: "The winged horse. Its Great Square of four stars is autumn's landmark — star-hop outward from its corners.", pos: [1298, 41.4], pts: [[20,10],[60,8],[64,46],[24,50],[86,62]], lines: [[0,1],[1,2],[2,3],[3,0],[2,4]] },
    { name: "Aquila", season: "Summer", fact: "The Eagle carrying Jupiter's thunderbolts. Altair, at its neck, is one corner of the Summer Triangle.", pos: [640, 72.9], pts: [[38,30],[30,20],[46,20],[10,6],[66,4],[36,52],[34,70]], lines: [[1,0],[0,2],[1,3],[2,4],[0,5],[5,6]] },
    { name: "Bootes", season: "Spring", fact: "The Herdsman, drawn as a kite. Follow the arc of the Big Dipper's handle and you arrive at golden Arcturus.", pos: [488, 30.5], pts: [[30,80],[12,50],[30,30],[50,48],[24,8],[46,14]], lines: [[0,1],[0,3],[1,2],[2,3],[2,4],[4,5],[5,3]] },
    { name: "Centaurus", season: "Spring", fact: "The Centaur, wrapped around the Southern Cross. Its front hoof, Alpha Centauri, is the nearest star system to us.", pos: [850, 63.6], pts: [[8,70],[22,62],[38,58],[52,48],[62,34],[74,22],[46,30],[34,16],[56,64]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[3,6],[6,7],[2,8]] },
    { name: "Carina", season: "Summer", fact: "The Keel of the ship Argo. Canopus, second-brightest star in the sky, sits along its hull.", pos: [1112, 33.1], pts: [[4,58],[22,50],[42,46],[60,52],[78,62],[52,26],[34,22]], lines: [[0,1],[1,2],[2,3],[3,4],[2,5],[5,6]] },
    { name: "Grus", season: "Autumn", fact: "The Crane, wading south of Fomalhaut — a long neck of stars stretched between two bright wings.", pos: [214, 76.1], pts: [[16,4],[24,20],[32,38],[28,56],[36,74],[8,44],[56,48]], lines: [[0,1],[1,2],[2,3],[3,4],[2,5],[2,6]] },
    { name: "Ursa Minor", season: "All year", fact: "The Little Bear. Polaris, the North Star, sits at the tip of its tail and barely moves all night.", pos: [990, 26.8], pts: [[50,6],[42,18],[36,30],[28,40],[14,36],[10,48],[24,52]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6],[6,3]] },
    { name: "Corona Borealis", season: "Summer", fact: "The Northern Crown — a small, tidy arc of seven stars set between Bootes and Hercules.", pos: [768, 5.6], pts: [[4,14],[14,32],[28,42],[44,44],[60,38],[72,26],[80,10]], lines: [[0,1],[1,2],[2,3],[3,4],[4,5],[5,6]] },
    { name: "Delphinus", season: "Autumn", fact: "The Dolphin, leaping out of the Milky Way. Four faint stars form Job's Coffin, with a tail trailing behind.", pos: [1258, 70.2], pts: [[24,10],[40,16],[36,32],[18,26],[8,44]], lines: [[0,1],[1,2],[2,3],[3,0],[3,4]] },
  ];
