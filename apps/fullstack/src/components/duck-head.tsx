import yawMinusEightPitchMinusFive from '../assets/duck/duck-yaw--8-pitch--5.png';
import yawZeroPitchMinusFive from '../assets/duck/duck-yaw-0-pitch--5.png';
import yawPlusEightPitchMinusFive from '../assets/duck/duck-yaw-8-pitch--5.png';
import yawMinusEightPitchZero from '../assets/duck/duck-yaw--8-pitch-0.png';
import yawZeroPitchZero from '../assets/duck/duck-yaw-0-pitch-0.png';
import yawPlusEightPitchZero from '../assets/duck/duck-yaw-8-pitch-0.png';
import yawMinusEightPitchFive from '../assets/duck/duck-yaw--8-pitch-5.png';
import yawZeroPitchFive from '../assets/duck/duck-yaw-0-pitch-5.png';
import yawPlusEightPitchFive from '../assets/duck/duck-yaw-8-pitch-5.png';

export function DuckHead() {
  return (
    <div
      id="plec-duck-head"
      className="duck-head h-12 w-12 aspect-square"
      aria-hidden="true"
    >
      <img
        className="duck-head-sprite"
        src={yawMinusEightPitchMinusFive}
        alt=""
        data-duck-yaw="-8"
        data-duck-pitch="-5"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawZeroPitchMinusFive}
        alt=""
        data-duck-yaw="0"
        data-duck-pitch="-5"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawPlusEightPitchMinusFive}
        alt=""
        data-duck-yaw="8"
        data-duck-pitch="-5"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawMinusEightPitchZero}
        alt=""
        data-duck-yaw="-8"
        data-duck-pitch="0"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawZeroPitchZero}
        alt=""
        data-duck-yaw="0"
        data-duck-pitch="0"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawPlusEightPitchZero}
        alt=""
        data-duck-yaw="8"
        data-duck-pitch="0"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawMinusEightPitchFive}
        alt=""
        data-duck-yaw="-8"
        data-duck-pitch="5"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawZeroPitchFive}
        alt=""
        data-duck-yaw="0"
        data-duck-pitch="5"
        draggable="false"
      />
      <img
        className="duck-head-sprite"
        src={yawPlusEightPitchFive}
        alt=""
        data-duck-yaw="8"
        data-duck-pitch="5"
        draggable="false"
      />
    </div>
  );
}
